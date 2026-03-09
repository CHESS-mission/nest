use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

use crate::component::{
    Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
    MessageDispatcher,
};
use crate::ports::{AnySystemPort, SystemPort};
use crate::{any_message_enum, with_message_wrapper};

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
}

// State

struct FuzzerState {
    /// Current select value
    fuzzing_select: bool,
}

impl Default for FuzzerState {
    fn default() -> Self {
        FuzzerState {
            fuzzing_select: false,
        }
    }
}
// Component

#[derive(Default)]
pub struct FuzzerComponent {
    fuzzed_ports: Vec<AnyFuzzedGroup>,
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
                // Create dispatcher
                let mut dispatcher = MessageDispatcher::<FuzzerState>::new();

                // Basic commands
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
                                    "set_fuzzing_select" => {
                                        if let Ok(fuzzing_select) =
                                            serde_json::from_value::<bool>(args)
                                        {
                                            state.fuzzing_select = fuzzing_select;
                                            Ok(())
                                        } else {
                                            Err(format!(
                                                "Bad arguments for simulator command {}: {}",
                                                command, args_string
                                            ))
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
                        }
                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register routing handlers
                for group in &self.fuzzed_ports {
                    with_message_wrapper!(AnyFuzzedGroup, group, |group| {
                        let syst_port = &group.syst_side;

                        // Man-in-the-middle routing
                        if let Some(comp_port) = &group.comp_side {
                            // Register comp->syst port handlers
                            dispatcher.register_system_port(
                                comp_port,
                                Box::new(move |msg, state| {
                                    if !state.fuzzing_select {
                                        syst_port.send(msg.clone());
                                    }
                                    Ok(HandlerStatus::Ok)
                                }),
                            );

                            // Register syst->comp port handlers
                            dispatcher.register_system_port(
                                syst_port,
                                Box::new(move |msg, state| {
                                    if !state.fuzzing_select {
                                        comp_port.send(msg.clone());
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
                dispatcher.start_dispatch(FuzzerState {
                    fuzzing_select: false,
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
            if group.syst_side.len() == 0 {
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
