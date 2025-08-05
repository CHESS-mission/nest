mod instance;
mod i2c;

// Re-exports
pub use crate::component::qemu::i2c::messaging::*;

use std::{path::PathBuf, time::Duration};

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    component::{
        qemu::{
            i2c::{start_i2c_backend},
            instance::{QemuConfig, QemuInstance},
        }, Component, ComponentMonitor, ComponentParams, HandlerStatus, MessageDispatcher, MonitorCommand
    },
    ports::{JsonMessage, SystemPort},
};

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
    #[serde(default = "default_i2c_timeout_ms")]
    pub i2c_timeout_ms: u64,
    pub i2c_socket: Option<String>,
}

fn default_i2c_timeout_ms() -> u64 {
    100
}

impl Default for QemuComponentConfig {
    fn default() -> Self {
        QemuComponentConfig {
            arch: "arm".into(),
            mem_mb: 1024,
            options: Vec::new(),
            i2c_timeout_ms: default_i2c_timeout_ms(),
            i2c_socket: None,
        }
    }
}

pub struct QemuComponentState {
    update_status: bool,
    qemu_instance: QemuInstance,
}

impl QemuComponentState {
    pub fn new(update_status: bool, qemu_instance: QemuInstance) -> Self {
        Self { update_status, qemu_instance }
    }
}

#[derive(Default)]
pub struct QemuComponent {
    config: QemuComponentConfig,
    i2c_port: SystemPort<JsonMessage>,
}

impl QemuComponent {
    pub fn new(config: QemuComponentConfig, i2c_port: SystemPort<JsonMessage>) -> Self {
        Self { config, i2c_port }
    }
}

impl Component for QemuComponent {
    fn start(self) -> Result<ComponentMonitor> {
        // Create i2c socket
        let i2c_socket = self.config.i2c_socket.as_ref().map(PathBuf::from).unwrap_or_else(||
            std::env::temp_dir().join(format!("vi2c-{}.sock", uuid::Uuid::new_v4()))
        );

        // Start i2c backend
        let i2c_backend_handle = start_i2c_backend(&i2c_socket, Duration::from_millis(self.config.i2c_timeout_ms));

        let monitor = ComponentMonitor::start_monitor(move |command_receiver, logger, _status| {
            let mut dispatcher = MessageDispatcher::<QemuComponentState>::new();

            // Register command receiver handler
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    MonitorCommand::Stop => return Ok(HandlerStatus::Stop),
                    MonitorCommand::StartStatusUpdates => state.update_status = true,
                    MonitorCommand::StopStatusUpdates => state.update_status = false,
                }

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c backend requests handler
            dispatcher.register(i2c_backend_handle.request_rx, Box::new(|req, _state| {
                let i2c_request = serde_json::to_value(req)?;
                self.i2c_port.send(&JsonMessage::new(i2c_request))?;

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c port handler
            dispatcher.register_system_port(&self.i2c_port, Box::new(|resp, _state| {
                let i2c_response = serde_json::from_value::<I2cResponse>(resp.data);
                match i2c_response {
                    Ok(response) => i2c_backend_handle.response_tx.send(response)?,
                    Err(_) => logger.error("Bad json received on i2c port")?,
                }

                Ok(HandlerStatus::Ok)
            }));

            // Configure qemu instance
            let mut qemu_config = QemuConfig::new(self.config.arch, self.config.mem_mb);
            for option in self.config.options {
                let opt_arg = format!("-{}", option.option);
                match option.value {
                    Some(value) => qemu_config.add_option(opt_arg, value),
                    None => qemu_config.add_flag(opt_arg),
                }
            }

            // Set i2c backend socket
            qemu_config.set_i2c_backend_socket(i2c_socket);

            // Start qemu instance
            let qemu_instance = QemuInstance::spawn(qemu_config)?;
            logger.info(&format!("Spawned QEMU instance: {}", qemu_instance.cmd_line()))?;

            // Start dispatcher
            let state = QemuComponentState::new(false, qemu_instance);
            dispatcher.start_dispatch(state)?;

            // Stop i2c backend
            let _ = i2c_backend_handle.join_handle.join();

            Ok(())
        });

        Ok(monitor)
    }

    fn from_params(params: &ComponentParams) -> Result<Self> where Self: Sized {
        let mut qemu_component = Self::default();

        // Parse config parameters
        qemu_component.config = serde_json::from_value::<QemuComponentConfig>(params.config().clone())?;

        // Get system ports from params
        for (key, value) in &params.system_ports {
            match key.as_str() {
                "i2c" => qemu_component.i2c_port = value.as_json()?.clone(),
                _ => bail!("Unknown system port {} for QemuComponent", key),
            }
        }

        // Return the fully initialized component
        Ok(qemu_component)
    }
}
