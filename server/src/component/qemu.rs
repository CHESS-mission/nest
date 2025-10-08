mod gpio;
mod i2c;
mod instance;

// Re-exports
pub use crate::component::qemu::i2c::messaging::*;

use std::{collections::HashMap, path::PathBuf, time::Duration};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::{
    component::{
        Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
        MessageDispatcher,
        qemu::{
            gpio::{GpioLine, messaging::GpioEvent, start_gpio_backend},
            i2c::start_i2c_backend,
            instance::{QemuConfig, QemuInstance},
        },
    },
    ports::{BoolMessage, JsonMessage, SystemPort},
};

#[derive(Serialize, Deserialize)]
pub enum GpioValue {
    Low,
    High,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QemuComponentConfigOption {
    pub option: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QemuComponentConfig {
    pub arch: String,
    mem_mb: u32,
    #[serde(default)]
    pub options: Vec<QemuComponentConfigOption>,
    pub gpio_socket: Option<String>,
    #[serde(default = "default_ngpio")]
    pub ngpio: u32,
    #[serde(default)]
    pub gpio_names: HashMap<String, String>,
    pub i2c_socket: Option<String>,
    #[serde(default = "default_i2c_timeout_ms")]
    pub i2c_timeout_ms: u64,
}

fn default_ngpio() -> u32 {
    32
}

fn default_i2c_timeout_ms() -> u64 {
    100
}

pub struct QemuComponentState {
    update_status: bool,
    qemu_instance: QemuInstance,
}

impl QemuComponentState {
    pub fn new(update_status: bool, qemu_instance: QemuInstance) -> Self {
        Self {
            update_status,
            qemu_instance,
        }
    }
}

pub struct QemuComponent {
    config: QemuComponentConfig,
    gpio_lines: Vec<GpioLine>,
    gpio_ports: Vec<SystemPort<BoolMessage>>,
    i2c_port: SystemPort<JsonMessage>,
}

impl QemuComponent {
    pub fn new(
        config: QemuComponentConfig,
        gpio_ports: Vec<SystemPort<BoolMessage>>,
        i2c_port: SystemPort<JsonMessage>,
    ) -> Result<Self> {
        let mut gpio_lines = vec![GpioLine::default(); config.ngpio as usize];
        for (i, name) in &config.gpio_names {
            let idx: usize = i.parse()?;
            gpio_lines
                .get_mut(idx)
                .ok_or_else(|| anyhow!("Gpio index {} out of range 0-{}", idx, config.ngpio))?
                .name = Some(name.clone());
        }

        Ok(Self {
            config,
            gpio_lines,
            gpio_ports,
            i2c_port,
        })
    }
}

impl Component for QemuComponent {
    fn start(self: Box<Self>) -> Result<ComponentHandle> {
        let handle =
            ComponentHandle::start(move |command_receiver, _event_sender, logger, _status| {
                // Create gpio socket
                let gpio_socket = self
                    .config
                    .gpio_socket
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::temp_dir().join(format!("vgpio-{}.sock", uuid::Uuid::new_v4()))
                    });

                // Start gpio backend
                let gpio_backend_handle = start_gpio_backend(&gpio_socket, self.gpio_lines);

                // Create i2c socket
                let i2c_socket = self
                    .config
                    .i2c_socket
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::temp_dir().join(format!("vi2c-{}.sock", uuid::Uuid::new_v4()))
                    });

                // Start i2c backend
                let i2c_backend_handle = start_i2c_backend(
                    &i2c_socket,
                    Duration::from_millis(self.config.i2c_timeout_ms),
                );

                // Configure qemu instance
                let mut qemu_config = QemuConfig::new(self.config.arch, self.config.mem_mb);
                for option in self.config.options {
                    let opt_arg = format!("-{}", option.option);
                    match option.value {
                        Some(value) => qemu_config.add_option(opt_arg, value),
                        None => qemu_config.add_flag(opt_arg),
                    }
                }

                // Set gpio backend socket
                qemu_config.set_gpio_backend_socket(gpio_socket);

                // Set i2c backend socket
                qemu_config.set_i2c_backend_socket(i2c_socket);

                // Start qemu instance
                let qemu_instance = QemuInstance::spawn(qemu_config)?;
                logger.info(format!(
                    "Spawned QEMU instance: {}",
                    qemu_instance.cmd_line()
                ))?;

                let mut dispatcher = MessageDispatcher::<QemuComponentState>::new();

                // Register command receiver handler
                dispatcher.register(
                    command_receiver,
                    Box::new(|msg, state| {
                        match msg {
                            ComponentHandleCommand::Stop => {
                                logger.info(format!("Stopping Qemu instance"))?;
                                state.qemu_instance.quit()?;
                                state.qemu_instance.wait()?;
                                logger.info(format!("Qemu instance stopped"))?;
                                return Ok(HandlerStatus::Stop);
                            }
                            ComponentHandleCommand::StartStatusUpdates => {
                                state.update_status = true
                            }
                            ComponentHandleCommand::StopStatusUpdates => {
                                state.update_status = false
                            }
                            ComponentHandleCommand::SimulatorCommand { command, args } => {
                                todo!();
                            }
                        }

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register gpio backend requests handler
                dispatcher.register(
                    gpio_backend_handle.events_rx,
                    Box::new(|req, _state| {
                        let (line, value) = match req {
                            GpioEvent::ValueLow(line) => (line, false),
                            GpioEvent::ValueHigh(line) => (line, true),
                        };

                        if let Some(port) = self.gpio_ports.get(line as usize) {
                            port.send(BoolMessage::new(value));
                        } else {
                            logger.error(format!("GPIO line out of range: {}", line))?;
                        }

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register gpio port handlers
                for (i, gpio_port) in self.gpio_ports.iter().enumerate() {
                    let gpio_backend_events_tx = &gpio_backend_handle.events_tx;
                    dispatcher.register_system_port(
                        gpio_port,
                        Box::new(move |msg, _state| {
                            let gpio_value_high = msg.data;

                            let event = if gpio_value_high {
                                GpioEvent::ValueHigh(i as u16)
                            } else {
                                GpioEvent::ValueLow(i as u16)
                            };

                            gpio_backend_events_tx.send(event)?;

                            Ok(HandlerStatus::Ok)
                        }),
                    );
                }

                // Register i2c backend requests handler
                dispatcher.register(
                    i2c_backend_handle.request_rx,
                    Box::new(|req, _state| {
                        let i2c_request = serde_json::to_value(req)?;
                        self.i2c_port.send(JsonMessage::new(i2c_request));

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Register i2c port handler
                dispatcher.register_system_port(
                    &self.i2c_port,
                    Box::new(|resp, _state| {
                        let i2c_response = serde_json::from_value::<I2cTransaction>(resp.data);
                        match i2c_response {
                            Ok(response) => i2c_backend_handle.response_tx.send(response)?,
                            Err(_) => logger.error("Bad json received on i2c port")?,
                        }

                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Start dispatcher
                let state = QemuComponentState::new(false, qemu_instance);
                dispatcher.start_dispatch(state)?;

                // Stop i2c backend
                let _ = i2c_backend_handle.join_handle.join();

                Ok(())
            });

        Ok(handle)
    }

    fn from_params(params: &ComponentParams) -> Result<Self>
    where
        Self: Sized,
    {
        // Parse config parameters
        let config = serde_json::from_value::<QemuComponentConfig>(params.config().clone())?;

        let mut gpio_ports = vec![SystemPort::default(); config.ngpio as usize];
        let mut i2c_port = SystemPort::default();

        // Get system ports from params
        for (key, value) in &params.system_ports {
            match key.as_str() {
                "i2c" => i2c_port = value.as_json()?.clone(),
                _ => {
                    if let Some(i) = key.strip_prefix("gpio") {
                        if let Ok(idx) = i.parse::<usize>() {
                            if let Some(gpio_port) = gpio_ports.get_mut(idx) {
                                *gpio_port = value.as_bool()?.clone();
                            } else {
                                bail!("Gpio port {} out of range 0-{}", key, config.ngpio);
                            }
                        } else {
                            bail!("Unknown system port {} for QemuComponent", key);
                        }
                    } else {
                        bail!("Unknown system port {} for QemuComponent", key);
                    }
                }
            }
        }

        // Return the fully initialized component
        QemuComponent::new(config, gpio_ports, i2c_port)
    }
}

// impl QemuComponent {
//     fn test(self: Box<Self>) -> Result<ComponentHandle> {
//         let gpio_socket = self.config.gpio_socket.as_ref().map(PathBuf::from).unwrap_or_else(||
//             std::env::temp_dir().join(format!("vgpio-{}.sock", uuid::Uuid::new_v4()))
//         );
//
//         let gpio_backend_handle = start_gpio_backend(&gpio_socket, self.gpio_lines);
//
//         let handle = ComponentHandle::start(move |command_receiver, _event_sender, logger, _status| {
//             let mut qemu_config = QemuConfig::new(self.config.arch, self.config.mem_mb);
//
//             Ok(())
//         });
//
//         Ok(handle)
//     }
// }
//
// impl Component for QemuComponent {
//
//     fn from_params(params: &ComponentParams) -> Result<Self> where Self: Sized {
//         // Parse config parameters
//         let config = serde_json::from_value::<QemuComponentConfig>(params.config().clone())?;
//
//         let mut gpio_ports = vec![SystemPort::default(); config.ngpio as usize];
//         let mut i2c_port = SystemPort::default();
//
//         // Get system ports from params
//         for (key, value) in &params.system_ports {
//             match key.as_str() {
//                 "i2c" => i2c_port = value.as_json()?.clone(),
//                 _ => {
//                     if let Some(i) = key.strip_prefix("gpio") {
//                         if let Ok(idx) = i.parse::<usize>() {
//                             if let Some(gpio_port) = gpio_ports.get_mut(idx) {
//                                 *gpio_port = value.as_bool()?.clone();
//                             } else {
//                                 bail!("Gpio port {} out of range 0-{}", key, config.ngpio);
//                             }
//                         } else {
//                             bail!("Unknown system port {} for QemuComponent", key);
//                         }
//                     } else {
//                         bail!("Unknown system port {} for QemuComponent", key);
//                     }
//                 },
//             }
//         }
//
//         // Return the fully initialized component
//         QemuComponent::new(config, gpio_ports, i2c_port)
//     }
//
//     fn start(self: Box<Self>) -> Result<ComponentHandle> {
//         todo!()
//     }
// }
