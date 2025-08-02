mod instance;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    thread::{JoinHandle, spawn},
    time::Duration,
};

use anyhow::{Result, anyhow, bail};

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::Deserialize;
use serde_json::json;
use vhost_user_backend::VhostUserDaemon;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};

use crate::{
    component::{
        qemu::instance::{QemuConfig, QemuInstance}, Component, ComponentMonitor, ComponentParams, HandlerStatus, MessageDispatcher, MonitorCommand
    },
    ports::{JsonMessage, SystemPort},
};

fn make_socket_path(stem: &str) -> Result<PathBuf> {
    let mut it = 0..65536;
    loop {
        let path = PathBuf::from(format!("/tmp/{}_{}.sock", stem, it.next().ok_or(anyhow!("Out of socket addresses."))?));
        if !path.try_exists()? {
            return Ok(path)
        }
    }
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

    fn start_i2c_backend<P: AsRef<Path>>(&self, rx: Receiver<Vec<u8>>, tx: Sender<I2cReq>, i2c_socket: P) -> Result<JoinHandle<Result<()>>> {
        let i2c_socket = i2c_socket.as_ref().to_owned();
        let timeout = Duration::from_millis(self.config.i2c_timeout_ms);

        let handle = spawn(move || {
            let backend = Arc::new(RwLock::new(
                VhostUserI2cBackend::new(rx, tx, timeout)?,
            ));

            let mut daemon = VhostUserDaemon::new(
                String::from("nest-vhost-i2c-backend"),
                backend.clone(),
                GuestMemoryAtomic::new(GuestMemoryMmap::new()),
            ).unwrap();

            daemon.serve(i2c_socket).unwrap();

            Ok(())
        });

        Ok(handle)
    }
}

impl Component for QemuComponent {
    fn start(self) -> Result<ComponentMonitor> {
        // Create i2c socket
        let i2c_socket = self.config.i2c_socket.as_ref().map(PathBuf::from).unwrap_or(make_socket_path("vi2c")?);

        // Start i2c backend
        let (i2c_backend_requests_tx, i2c_backend_requests_rx) = unbounded();
        let (i2c_backend_replies_tx, i2c_backend_replies_rx) = unbounded();
        let i2c_backend_handle = self.start_i2c_backend(i2c_backend_replies_rx, i2c_backend_requests_tx, &i2c_socket)?;

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
            dispatcher.register(i2c_backend_requests_rx, Box::new(|req, _state| {
                let i2c_request = json!({
                    "addr": req.addr,
                    "read": req.flags == I2C_M_RD,
                    "len": req.len,
                    "buf": req.buf,
                });
                self.i2c_port.send(&JsonMessage::new(i2c_request))?;

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c port handler
            dispatcher.register_system_port(&self.i2c_port, Box::new(|msg, _state| {
                #[derive(Deserialize)]
                struct I2cResponse {
                    buf: Vec<u8>,
                }

                let i2c_response = serde_json::from_value::<I2cResponse>(msg.data);
                match i2c_response {
                    Ok(response) => i2c_backend_replies_tx.send(response.buf)?,
                    Err(_) => logger.error("Bad json received on i2c port")?,
                }

                Ok(HandlerStatus::Ok)
            }));

            // Configure qemu instance
            let mut qemu_config = QemuConfig::new(self.config.arch, self.config.mem_mb);
            for option in &self.config.options {
                match option.value {
                    Some(value) => qemu_config.add_option(option.option, value),
                    None => qemu_config.add_flag(option.option),
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

            // Stop the backend
            i2c_backend_handle.join().unwrap()?;

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
