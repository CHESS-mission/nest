use std::collections::HashMap;
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
struct I2cConfig {
    speed: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UartConfig {
    baudrate: f32,
    stop_bits: String,
    parity: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HilConfig {
    #[serde(default)]
    i2c: HashMap<String, I2cConfig>,
    uart: HashMap<String, UartConfig>,
}

// State

struct HilState {
    connected: bool,
}

// Component

#[derive(Default)]
pub struct HilComponent {
    gpio_ports: HashMap<String, BoolMessage>,
    i2c_ports: HashMap<String, Bytes>,
    uart_ports: HashMap<String, Bytes>,
}

#[derive(Clone, Debug)]
struct MultiplexedGroup<M: Clone + Send + Sync + 'static> {
    common: SystemPort<M>,
    channels: Vec<Option<SystemPort<M>>>,
}

impl<M: Clone + Send + Sync + 'static> MultiplexedGroup<M> {
    fn new(common: SystemPort<M>, channels: Vec<Option<SystemPort<M>>>) -> Self {
        Self { common, channels }
    }
}

any_message_enum!(AnyMultiplexedGroup, MultiplexedGroup);

impl Component for HilComponent {
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
                        if let Some(channel) = &group.channels[state.select as usize] {
                            channel.send(msg);
                        }
                        Ok(HandlerStatus::Ok)
                    }));

                    // Register channel port handlers
                    for (i, channel) in group.channels.iter().enumerate() {
                        if let Some(channel) = channel {
                            dispatcher.register_system_port(channel, Box::new(move |msg, state| {
                                if state.select as usize == i {
                                    group.common.send(msg);
                                }
                                Ok(HandlerStatus::Ok)
                            }));
                        }
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

            if let Some(common_port) = params.system_ports.remove(&group.common) {
                with_message_wrapper!(AnySystemPort, common_port, |common_port| {
                    let channels = group.channels.iter()
                        .map(|channel| {
                            params.system_ports.remove(channel)
                                .map(|port| port.try_into().map_err(|_| anyhow::anyhow!("Common and channel port types must match")))
                                .transpose()
                        })
                        .collect::<anyhow::Result<_>>()?;
                    comp.multiplexed_ports.push(MultiplexedGroup::new(common_port, channels).into());
                });
            } else {
                // Remove channels if no common
                for channel in &group.channels {
                    params.system_ports.remove(channel);
                }
            }
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

