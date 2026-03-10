mod engine;

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

use crate::component::{
    Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
    MessageDispatcher,
};
use crate::ports::{AnySystemPort, SystemPort};
use crate::{any_message_enum, with_message_wrapper};

use engine::{Direction, FuzzEngine, FuzzProfile};

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
    comp_to_syst: FuzzProfile,
    #[serde(default)]
    syst_to_comp: FuzzProfile,
    seed: Option<u64>,
}

// State

struct FuzzerState {
    /// One FuzzEngine per group, keyed by group name.
    engines: HashMap<String, FuzzEngine>,
}

// Component

#[derive(Default)]
pub struct FuzzerComponent {
    fuzzed_ports: Vec<AnyFuzzedGroup>,
    initial_engines: Vec<FuzzEngine>,
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

                // Create dispatcher
                let mut dispatcher = MessageDispatcher::<FuzzerState>::new();

                // Basic commands + simulator commands
                dispatcher.register(
                    command_receiver,
                    Box::new(|msg, state| {
                        match msg {
                            ComponentHandleCommand::Stop => {
                                logger.info("Stopping fuzzer component")?;
                                return Ok(HandlerStatus::Stop);
                            }
                            ComponentHandleCommand::StartStatusUpdates => {}
                            ComponentHandleCommand::StopStatusUpdates => {}
                            ComponentHandleCommand::SimulatorCommand { command, args } => {
                                let args_string = args.to_string();
                                let result = match command.as_str() {
                                    // ------------------------------------------------
                                    // set_profile { group, direction, ...profile }
                                    // ------------------------------------------------
                                    "set_profile" => {
                                        #[derive(Deserialize)]
                                        struct SetProfileArgs {
                                            group: String,
                                            direction: String,
                                            #[serde(flatten)]
                                            profile: FuzzProfile,
                                        }
                                        match serde_json::from_value::<SetProfileArgs>(args) {
                                            Ok(a) => {
                                                if let Some(engine) =
                                                    state.engines.get_mut(&a.group)
                                                {
                                                    match a.direction.as_str() {
                                                        "comp_to_syst" => {
                                                            engine.comp_to_syst = a.profile;
                                                        }
                                                        "syst_to_comp" => {
                                                            engine.syst_to_comp = a.profile;
                                                        }
                                                        "both" => {
                                                            engine.comp_to_syst = a.profile.clone();
                                                            engine.syst_to_comp = a.profile;
                                                        }
                                                        _ => {
                                                            return Ok(HandlerStatus::Ok);
                                                        }
                                                    }
                                                    Ok(())
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => {
                                                Err(format!("Bad arguments for {}: {}", command, e))
                                            }
                                        }
                                    }
                                    // ------------------------------------------------
                                    // set_enabled { group, direction, enabled }
                                    // ------------------------------------------------
                                    "set_enabled" => {
                                        #[derive(Deserialize)]
                                        struct SetEnabledArgs {
                                            group: String,
                                            direction: String,
                                            enabled: bool,
                                        }
                                        match serde_json::from_value::<SetEnabledArgs>(args) {
                                            Ok(a) => {
                                                if let Some(engine) =
                                                    state.engines.get_mut(&a.group)
                                                {
                                                    let dirs: Vec<Direction> =
                                                        match a.direction.as_str() {
                                                            "comp_to_syst" => {
                                                                vec![Direction::CompToSyst]
                                                            }
                                                            "syst_to_comp" => {
                                                                vec![Direction::SystToComp]
                                                            }
                                                            "both" => vec![
                                                                Direction::CompToSyst,
                                                                Direction::SystToComp,
                                                            ],
                                                            _ => vec![],
                                                        };
                                                    for dir in dirs {
                                                        engine.profile_mut(dir).enabled = a.enabled;
                                                    }
                                                    Ok(())
                                                } else {
                                                    Err(format!("Unknown fuzz group: {}", a.group))
                                                }
                                            }
                                            Err(e) => {
                                                Err(format!("Bad arguments for {}: {}", command, e))
                                            }
                                        }
                                    }
                                    _ => Err(format!("Unknown simulator command: {}", command)),
                                };

                                match result {
                                    Ok(()) => logger.info(format!(
                                        "Received simulator command: {} {}",
                                        command, args_string
                                    ))?,
                                    Err(err) => logger.error(&err)?,
                                };
                            }
                        };

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register routing handlers
                for group in &self.fuzzed_ports {
                    with_message_wrapper!(AnyFuzzedGroup, group, |group| {
                        let group_name = group.name.clone();
                        let syst_port = &group.syst_side;

                        // Man-in-the-middle routing
                        if let Some(comp_port) = &group.comp_side {
                            // comp -> syst
                            let name = group_name.clone();
                            dispatcher.register_system_port(
                                comp_port,
                                Box::new(move |msg, state| {
                                    if let Some(engine) = state.engines.get_mut(&name) {
                                        if let Some(msg) =
                                            engine.process(msg, Direction::CompToSyst)
                                        {
                                            syst_port.send(msg);
                                        }
                                    }
                                    Ok(HandlerStatus::Ok)
                                }),
                            );

                            // syst -> comp
                            let name = group_name;
                            dispatcher.register_system_port(
                                syst_port,
                                Box::new(move |msg, state| {
                                    if let Some(engine) = state.engines.get_mut(&name) {
                                        if let Some(msg) =
                                            engine.process(msg, Direction::SystToComp)
                                        {
                                            comp_port.send(msg);
                                        }
                                    }
                                    Ok(HandlerStatus::Ok)
                                }),
                            );
                        // Component impersonator if no comp port
                        } else {
                            //TODO
                        }
                    });
                }

                // Start dispatch
                dispatcher.start_dispatch(FuzzerState { engines })?;

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

                // Build the FuzzEngine for this group
                comp.initial_engines.push(FuzzEngine::new(
                    group.name.clone(),
                    group.comp_to_syst.clone(),
                    group.syst_to_comp.clone(),
                    group.seed,
                ));
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
