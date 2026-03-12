mod engine;
mod injection;
mod strategies;
mod strategy;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

use crate::component::{
    Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
    MessageDispatcher,
};
use crate::ports::{AnySystemPort, SystemPort};
use crate::{any_message_enum, with_message_wrapper};

use engine::{Direction, FuzzEngine, FuzzProfileConfig};
use injection::{InjectionConfig, MessageGenerator};

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuzzerConfig {
    #[serde(default)]
    fuzzed_ports: Vec<FuzzedGroupSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FuzzedGroupSpec {
    name: String,
    syst_side: String,
    comp_side: String,
    #[serde(default)]
    comp_to_syst: FuzzProfileConfig,
    #[serde(default)]
    syst_to_comp: FuzzProfileConfig,
    #[serde(default)]
    injection: InjectionConfig,
    seed: Option<u64>,
}

// State

struct FuzzerState {
    /// One FuzzEngine per group, keyed by group name.
    engines: HashMap<String, FuzzEngine>,
    /// Injection configs per group (mutable at runtime).
    injection_configs: HashMap<String, InjectionConfig>,
}

// Component

#[derive(Default)]
pub struct FuzzerComponent {
    fuzzed_ports: Vec<AnyFuzzedGroup>,
    initial_engines: Vec<FuzzEngine>,
    injection_specs: Vec<(String, InjectionConfig)>,
}

#[derive(Clone, Debug)]
struct FuzzedGroup<M: Clone + Send + Sync + 'static> {
    name: String,
    syst_side: SystemPort<M>,
    comp_side: Option<SystemPort<M>>,
}

impl<M: Clone + Send + Sync + 'static> FuzzedGroup<M> {
    fn new(name: String, syst_side: SystemPort<M>, comp_side: Option<SystemPort<M>>) -> Self {
        Self {
            name,
            syst_side,
            comp_side,
        }
    }
}

any_message_enum!(AnyFuzzedGroup, FuzzedGroup);

/// Helper to parse a direction string into Direction values.
fn parse_directions(s: &str) -> Vec<Direction> {
    match s {
        "comp_to_syst" => vec![Direction::CompToSyst],
        "syst_to_comp" => vec![Direction::SystToComp],
        "both" => vec![Direction::CompToSyst, Direction::SystToComp],
        _ => vec![],
    }
}

impl Component for FuzzerComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        let handle =
            ComponentHandle::start(move |command_receiver, event_sender, logger, _status| {
                // Build initial state from the engines created in from_params
                let engines: HashMap<String, FuzzEngine> = self
                    .initial_engines
                    .into_iter()
                    .map(|e| (e.name.clone(), e))
                    .collect();

                let injection_configs: HashMap<String, InjectionConfig> = self
                    .injection_specs
                    .into_iter()
                    .collect();

                // Create dispatcher
                let mut dispatcher = MessageDispatcher::<FuzzerState>::new();

                // Basic commands + simulator commands
                let cmd_logger = logger.clone();
                dispatcher.register(
                    command_receiver,
                    Box::new(move |msg, state| {
                        match msg {
                            ComponentHandleCommand::Stop => {
                                cmd_logger.info("Stopping fuzzer component")?;
                                return Ok(HandlerStatus::Stop);
                            }
                            ComponentHandleCommand::StartStatusUpdates => {}
                            ComponentHandleCommand::StopStatusUpdates => {}
                            ComponentHandleCommand::SimulatorCommand { command, args } => {
                                let args_string = args.to_string();
                                let result = match command.as_str() {
                                    // ------------------------------------------------
                                    // set_enabled { group, direction, enabled }
                                    // ------------------------------------------------
                                    "set_enabled" => {
                                        #[derive(Deserialize)]
                                        struct Args {
                                            group: String,
                                            direction: String,
                                            enabled: bool,
                                        }
                                        match serde_json::from_value::<Args>(args) {
                                            Ok(a) => {
                                                if let Some(engine) =
                                                    state.engines.get_mut(&a.group)
                                                {
                                                    for dir in parse_directions(&a.direction) {
                                                        engine.profile_mut(dir).enabled = a.enabled;
                                                    }
                                                    Ok(())
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => Err(format!("Bad arguments: {}", e)),
                                        }
                                    }
                                    // ------------------------------------------------
                                    // set_strategy_enabled { group, direction, strategy, enabled }
                                    // ------------------------------------------------
                                    "set_strategy_enabled" => {
                                        #[derive(Deserialize)]
                                        struct Args {
                                            group: String,
                                            direction: String,
                                            strategy: String,
                                            enabled: bool,
                                        }
                                        match serde_json::from_value::<Args>(args) {
                                            Ok(a) => {
                                                if let Some(engine) =
                                                    state.engines.get_mut(&a.group)
                                                {
                                                    let mut found = false;
                                                    for dir in parse_directions(&a.direction) {
                                                        for s in &mut engine.profile_mut(dir).strategies {
                                                            if s.name() == a.strategy {
                                                                s.set_enabled(a.enabled);
                                                                found = true;
                                                            }
                                                        }
                                                    }
                                                    if found {
                                                        Ok(())
                                                    } else {
                                                        Err(format!(
                                                            "Strategy '{}' not found in group '{}'",
                                                            a.strategy, a.group
                                                        ))
                                                    }
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => Err(format!("Bad arguments: {}", e)),
                                        }
                                    }
                                    // ------------------------------------------------
                                    // list_strategies { group, direction }
                                    // ------------------------------------------------
                                    "list_strategies" => {
                                        #[derive(Deserialize)]
                                        struct Args {
                                            group: String,
                                            direction: String,
                                        }
                                        match serde_json::from_value::<Args>(args) {
                                            Ok(a) => {
                                                if let Some(engine) =
                                                    state.engines.get_mut(&a.group)
                                                {
                                                    for dir in parse_directions(&a.direction) {
                                                        let names: Vec<String> = engine
                                                            .profile(dir)
                                                            .strategies
                                                            .iter()
                                                            .map(|s| {
                                                                format!(
                                                                    "{}({})",
                                                                    s.name(),
                                                                    if s.enabled() {
                                                                        "on"
                                                                    } else {
                                                                        "off"
                                                                    }
                                                                )
                                                            })
                                                            .collect();
                                                        cmd_logger.info(format!(
                                                            "[{}] {:?} strategies: [{}]",
                                                            a.group,
                                                            dir,
                                                            names.join(", ")
                                                        ))?;
                                                    }
                                                    Ok(())
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => Err(format!("Bad arguments: {}", e)),
                                        }
                                    }
                                    // ------------------------------------------------
                                    // set_injection { group, enabled, interval_ms, ... }
                                    // ------------------------------------------------
                                    "set_injection" => {
                                        #[derive(Deserialize)]
                                        struct Args {
                                            group: String,
                                            #[serde(flatten)]
                                            config: InjectionConfig,
                                        }
                                        match serde_json::from_value::<Args>(args) {
                                            Ok(a) => {
                                                if state.engines.contains_key(&a.group) {
                                                    state
                                                        .injection_configs
                                                        .insert(a.group, a.config);
                                                    Ok(())
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => Err(format!("Bad arguments: {}", e)),
                                        }
                                    }
                                    _ => Err(format!("Unknown simulator command: {}", command)),
                                };

                                match result {
                                    Ok(()) => cmd_logger.info(format!(
                                        "Received simulator command: {} {}",
                                        command, args_string
                                    ))?,
                                    Err(err) => cmd_logger.error(&err)?,
                                };
                            }
                        };

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register routing handlers
                for group in &self.fuzzed_ports {
                    // We need to match on the concrete AnyFuzzedGroup variants to get
                    // type-specific dispatch (Bytes → process_bytes, I2c → process_i2c, others → process_generic).
                    match group {
                        AnyFuzzedGroup::Bytes(group) => {
                            let group_name = group.name.clone();
                            let syst_port = &group.syst_side;

                            if let Some(comp_port) = &group.comp_side {
                                // comp -> syst
                                let name = group_name.clone();
                                let log = logger.clone();
                                dispatcher.register_system_port(
                                    comp_port,
                                    Box::new(move |msg, state| {
                                        if let Some(engine) = state.engines.get_mut(&name) {
                                            let (result, logs) =
                                                engine.process_bytes(msg, Direction::CompToSyst);
                                            for entry in logs {
                                                log.info(format!(
                                                    "[{}] comp->syst: {}",
                                                    name, entry
                                                ))?;
                                            }
                                            if let Some(msg) = result {
                                                syst_port.send(msg);
                                            }
                                        }
                                        Ok(HandlerStatus::Ok)
                                    }),
                                );

                                // syst -> comp
                                let name = group_name;
                                let log = logger.clone();
                                dispatcher.register_system_port(
                                    syst_port,
                                    Box::new(move |msg, state| {
                                        if let Some(engine) = state.engines.get_mut(&name) {
                                            let (result, logs) =
                                                engine.process_bytes(msg, Direction::SystToComp);
                                            for entry in logs {
                                                log.info(format!(
                                                    "[{}] syst->comp: {}",
                                                    name, entry
                                                ))?;
                                            }
                                            if let Some(msg) = result {
                                                comp_port.send(msg);
                                            }
                                        }
                                        Ok(HandlerStatus::Ok)
                                    }),
                                );
                            }
                        }
                        AnyFuzzedGroup::I2c(group) => {
                            let group_name = group.name.clone();
                            let syst_port = &group.syst_side;

                            if let Some(comp_port) = &group.comp_side {
                                // comp -> syst
                                let name = group_name.clone();
                                let log = logger.clone();
                                dispatcher.register_system_port(
                                    comp_port,
                                    Box::new(move |msg, state| {
                                        if let Some(engine) = state.engines.get_mut(&name) {
                                            let (result, logs) =
                                                engine.process_i2c(msg, Direction::CompToSyst);
                                            for entry in logs {
                                                log.info(format!(
                                                    "[{}] comp->syst: {}",
                                                    name, entry
                                                ))?;
                                            }
                                            if let Some(msg) = result {
                                                syst_port.send(msg);
                                            }
                                        }
                                        Ok(HandlerStatus::Ok)
                                    }),
                                );

                                // syst -> comp
                                let name = group_name;
                                let log = logger.clone();
                                dispatcher.register_system_port(
                                    syst_port,
                                    Box::new(move |msg, state| {
                                        if let Some(engine) = state.engines.get_mut(&name) {
                                            let (result, logs) =
                                                engine.process_i2c(msg, Direction::SystToComp);
                                            for entry in logs {
                                                log.info(format!(
                                                    "[{}] syst->comp: {}",
                                                    name, entry
                                                ))?;
                                            }
                                            if let Some(msg) = result {
                                                comp_port.send(msg);
                                            }
                                        }
                                        Ok(HandlerStatus::Ok)
                                    }),
                                );
                            }
                        }
                        // For all other message types (Int, Float, Bool, Text): generic passthrough with drop-only
                        _ => {
                            with_message_wrapper!(AnyFuzzedGroup, group, |group| {
                                let group_name = group.name.clone();
                                let syst_port = &group.syst_side;

                                if let Some(comp_port) = &group.comp_side {
                                    // comp -> syst
                                    let name = group_name.clone();
                                    let log = logger.clone();
                                    dispatcher.register_system_port(
                                        comp_port,
                                        Box::new(move |msg, state| {
                                            if let Some(engine) = state.engines.get_mut(&name) {
                                                let (result, logs) = engine
                                                    .process_generic(msg, Direction::CompToSyst);
                                                for entry in logs {
                                                    log.info(format!(
                                                        "[{}] comp->syst: {}",
                                                        name, entry
                                                    ))?;
                                                }
                                                if let Some(msg) = result {
                                                    syst_port.send(msg);
                                                }
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );

                                    // syst -> comp
                                    let name = group_name;
                                    let log = logger.clone();
                                    dispatcher.register_system_port(
                                        syst_port,
                                        Box::new(move |msg, state| {
                                            if let Some(engine) = state.engines.get_mut(&name) {
                                                let (result, logs) = engine
                                                    .process_generic(msg, Direction::SystToComp);
                                                for entry in logs {
                                                    log.info(format!(
                                                        "[{}] syst->comp: {}",
                                                        name, entry
                                                    ))?;
                                                }
                                                if let Some(msg) = result {
                                                    comp_port.send(msg);
                                                }
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );
                                }
                            });
                        }
                    }
                }

                // Register injection tickers for groups with injection enabled.
                // We iterate over the fuzzed_ports again to get port references for injection.
                for group in &self.fuzzed_ports {
                    match group {
                        AnyFuzzedGroup::Bytes(group) => {
                            let group_name = group.name.clone();
                            if let Some(config) = injection_configs.get(&group_name) {
                                if config.enabled && config.interval_ms > 0 {
                                    let ticker = crossbeam_channel::tick(
                                        Duration::from_millis(config.interval_ms),
                                    );
                                    let syst_port = group.syst_side.clone();
                                    let comp_port = group.comp_side.clone();
                                    let name = group_name.clone();
                                    let cfg_direction = config.direction.clone();
                                    let min = config.payload_len_min;
                                    let max = config.payload_len_max;
                                    let log = logger.clone();

                                    dispatcher.register(
                                        ticker,
                                        Box::new(move |_instant, state| {
                                            let inj = state.injection_configs.get(&name);
                                            if !inj.map_or(false, |c| c.enabled) {
                                                return Ok(HandlerStatus::Ok);
                                            }
                                            if let Some(engine) = state.engines.get_mut(&name) {
                                                let msg = MessageGenerator::random_bytes(
                                                    &mut engine.rng,
                                                    min,
                                                    max,
                                                );
                                                let dirs = parse_directions(&cfg_direction);
                                                for dir in &dirs {
                                                    match dir {
                                                        Direction::CompToSyst => {
                                                            syst_port.send(msg.clone());
                                                        }
                                                        Direction::SystToComp => {
                                                            if let Some(ref cp) = comp_port {
                                                                cp.send(msg.clone());
                                                            }
                                                        }
                                                    }
                                                }
                                                log.info(format!(
                                                    "[{}] injected phantom bytes ({} bytes)",
                                                    name,
                                                    msg.len()
                                                ))?;
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );
                                }
                            }
                        }
                        AnyFuzzedGroup::I2c(group) => {
                            let group_name = group.name.clone();
                            if let Some(config) = injection_configs.get(&group_name) {
                                if config.enabled && config.interval_ms > 0 {
                                    let ticker = crossbeam_channel::tick(
                                        Duration::from_millis(config.interval_ms),
                                    );
                                    let syst_port = group.syst_side.clone();
                                    let comp_port = group.comp_side.clone();
                                    let name = group_name.clone();
                                    let cfg_direction = config.direction.clone();
                                    let min = config.payload_len_min;
                                    let max = config.payload_len_max;
                                    let log = logger.clone();

                                    dispatcher.register(
                                        ticker,
                                        Box::new(move |_instant, state| {
                                            let inj = state.injection_configs.get(&name);
                                            if !inj.map_or(false, |c| c.enabled) {
                                                return Ok(HandlerStatus::Ok);
                                            }
                                            if let Some(engine) = state.engines.get_mut(&name) {
                                                let msg = MessageGenerator::random_i2c(
                                                    &mut engine.rng,
                                                    min,
                                                    max,
                                                );
                                                let dirs = parse_directions(&cfg_direction);
                                                for dir in &dirs {
                                                    match dir {
                                                        Direction::CompToSyst => {
                                                            syst_port.send(msg.clone());
                                                        }
                                                        Direction::SystToComp => {
                                                            if let Some(ref cp) = comp_port {
                                                                cp.send(msg.clone());
                                                            }
                                                        }
                                                    }
                                                }
                                                log.info(format!(
                                                    "[{}] injected phantom I2C message: {:?}",
                                                    name, msg
                                                ))?;
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );
                                }
                            }
                        }
                        // No injection for other port types (Int, Float, Bool, Text)
                        _ => {}
                    }
                }

                // Start dispatch
                dispatcher.start_dispatch(FuzzerState {
                    engines,
                    injection_configs,
                })?;

                // event_sender is unused; keep it so the signature matches your framework
                let _ = event_sender;
                Ok(())
            });

        Ok(handle)
    }

    fn from_params(mut params: ComponentParams) -> Result<Self>
    where
        Self: Sized,
    {
        let mut comp = Self::default();

        let config = serde_json::from_value::<FuzzerConfig>(params.config().clone())?;

        for group in &config.fuzzed_ports {
            if group.syst_side.is_empty() {
                bail!(
                    "Fuzzed group {} must have at least one system-side port",
                    group.name
                );
            }

            if let Some(syst_port) = params.system_ports.remove(&group.syst_side) {
                with_message_wrapper!(AnySystemPort, syst_port, |syst_port| {
                    let comp_port = if let Some(comp_port) =
                        params.system_ports.remove(&group.comp_side)
                    {
                        let comp_port = comp_port.try_into().map_err(|_| {
                            anyhow::anyhow!("The ports in a fuzzed group must have the same type")
                        })?;
                        Some(comp_port)
                    } else {
                        None
                    };

                    comp.fuzzed_ports
                        .push(FuzzedGroup::new(group.name.clone(), syst_port, comp_port).into());
                });

                // Build the FuzzEngine for this group with strategy pipelines
                let comp_to_syst_profile = group.comp_to_syst.build()?;
                let syst_to_comp_profile = group.syst_to_comp.build()?;

                comp.initial_engines.push(FuzzEngine::new(
                    group.name.clone(),
                    comp_to_syst_profile,
                    syst_to_comp_profile,
                    group.seed,
                ));

                // Store injection config for this group
                comp.injection_specs
                    .push((group.name.clone(), group.injection.clone()));
            } else {
                // Remove comp port if no syst port for this group
                params.system_ports.remove(&group.comp_side);
            }
        }

        // Check for remaining unknown ports
        if let Some((name, _port)) = params.system_ports.iter().next() {
            bail!("Unknown system port {} for FuzzerComponent", name);
        }

        Ok(comp)
    }
}
