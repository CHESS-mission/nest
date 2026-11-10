use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;

use crate::ports::{AnySystemPort, BoolMessage, SystemPort};
use crate::{any_message_enum, with_message_wrapper};
use crate::
    component::{
        Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
        MessageDispatcher,
    }
;

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MuxConfig {
    #[serde(default)]
    multiplexed_ports: Vec<MultiplexedGroupSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiplexedGroupSpec {
    common: String,
    channels: Vec<String>,
}

// State

struct MuxState {
    /// Current select value
    select: bool,
}

// Component

#[derive(Default)]
pub struct MuxComponent {
    select_port: SystemPort<BoolMessage>,
    multiplexed_ports: Vec<AnyMultiplexedGroup>,
}

#[derive(Clone, Debug)]
pub struct MultiplexedGroup<M: Clone + Send + Sync + 'static> {
    common: SystemPort<M>,
    channels: Vec<SystemPort<M>>,
}

impl<M: Clone + Send + Sync + 'static> MultiplexedGroup<M> {
    fn new(common: SystemPort<M>, channels: Vec<SystemPort<M>>) -> Self {
        Self { common, channels }
    }
}

any_message_enum!(AnyMultiplexedGroup, MultiplexedGroup);

impl Component for MuxComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, _status| {
            // Create dispatcher
            let mut dispatcher = MessageDispatcher::<MuxState>::new();

            // Basic commands
            dispatcher.register(command_receiver, Box::new(|msg, _state| {
                match msg {
                    ComponentHandleCommand::Stop => {
                        logger.info("Stopping multiplexer component")?;
                        return Ok(HandlerStatus::Stop);
                    }
                    ComponentHandleCommand::StartStatusUpdates => {}
                    ComponentHandleCommand::StopStatusUpdates => {}
                    ComponentHandleCommand::SimulatorCommand { command, .. } => {
                        logger.error(format!("Mux does not support simulator command: {command}"))?;
                    }
                }
                Ok(HandlerStatus::Ok)
            }));

            // Register select port handler
            dispatcher.register_system_port(&self.select_port, Box::new(|msg, state| {
                logger.info(format!("Selecting channel {}", msg as usize))?;
                state.select = msg;
                Ok(HandlerStatus::Ok)
            }));

            // Register routing handlers
            for group in &self.multiplexed_ports {
                with_message_wrapper!(AnyMultiplexedGroup, group, |group| {
                    // Register common port handler
                    dispatcher.register_system_port(&group.common, Box::new(move |msg, state| {
                        group.channels[state.select as usize].send(msg);
                        Ok(HandlerStatus::Ok)
                    }));

                    // Register channel port handlers
                    for (i, channel) in group.channels.iter().enumerate() {
                        dispatcher.register_system_port(channel, Box::new(move |msg, state| {
                            if state.select as usize == i {
                                group.common.send(msg);
                            }
                            Ok(HandlerStatus::Ok)
                        }));
                    }
                });
            }

            // Start dispatch
            dispatcher.start_dispatch(MuxState { select: false })?;

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

        let config = serde_json::from_value::<MuxConfig>(params.config().clone())?;

        for group in &config.multiplexed_ports {
            if group.channels.len() != 2 {
                bail!("Every multiplexed port must have exactly 2 channels");
            }

            let Some(common_port) = params.system_ports.remove(&group.common) else {
                bail!("Multiplexed port {} is not declared as a system port of this component", group.common);
            };

            with_message_wrapper!(AnySystemPort, common_port, |common_port| {
                let channels = group.channels.iter()
                    .map(|channel| {
                        let Some(port) = params.system_ports.remove(channel) else {
                            bail!("Channel port {} of multiplexed port {} is not declared as a system port",
                                channel, group.common);
                        };
                        let converted: SystemPort<M> = port.try_into()
                            .map_err(|_| anyhow!("Common and channel port types must match"))?;
                        Ok(converted)
                    })
                    .collect::<anyhow::Result<_>>()?;
                comp.multiplexed_ports.push(MultiplexedGroup::new(common_port, channels).into());
            });
        }

        if let Some(port) = params.system_ports.remove("select") {
            comp.select_port = port.into_bool()?;
        }

        // Check for remaining unknown ports
        if let Some((name, _port)) = params.system_ports.iter().next() {
            bail!("Unknown system port {} for MuxComponent", name);
        }

        Ok(comp)
    }
}

