mod instance;
mod uart;
mod gpio;
mod i2c;

use std::{collections::HashMap, path::PathBuf, time::{Duration, Instant}};

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, Result, bail};

use crate::{
    component::{
        Component, ComponentEvent, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus, Logger, MessageDispatcher, qemu::{
            gpio::{GpioLine, messaging::GpioEvent, start_gpio_backend}, i2c::start_i2c_backend, instance::{QemuConfig, QemuInstance}, uart::{UartPort, UartTransaction, start_uart_backend}
        }
    },
    ports::{BoolMessage, BytesMessage, I2cMessage, SystemPort},
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
    pub gpio_names: HashMap<u32, String>,
    pub i2c_socket: Option<String>,
    #[serde(default = "default_i2c_timeout_ms")]
    pub i2c_timeout_ms: u64,
    #[serde(default)]
    pub nuart: u32,
    #[serde(default)]
    pub uart_sockets: HashMap<String, String>,
}

fn default_ngpio() -> u32 { 32 }
fn default_i2c_timeout_ms() -> u64 { 100 }

pub struct QemuComponentState {
    update_status: bool,
    qemu_instance: QemuInstance,
}

impl QemuComponentState {
    pub fn new(update_status: bool, qemu_instance: QemuInstance) -> Self {
        Self { update_status, qemu_instance }
    }
}

pub struct QemuComponent {
    config: QemuComponentConfig,
    uart_ports: Vec<SystemPort<BytesMessage>>,
    gpio_lines: Vec<GpioLine>,
    gpio_ports: Vec<SystemPort<BoolMessage>>,
    i2c_port: SystemPort<I2cMessage>,
}

impl QemuComponent {
    pub fn new(
        config: QemuComponentConfig,
        uart_ports: Vec<SystemPort<BytesMessage>>,
        gpio_ports: Vec<SystemPort<BoolMessage>>,
        i2c_port: SystemPort<I2cMessage>
    ) -> Result<Self> {
        let mut gpio_lines = vec![GpioLine::default(); config.ngpio as usize];
        for (i, name) in &config.gpio_names {
            gpio_lines.get_mut(*i as usize)
                .ok_or_else(|| anyhow!("Gpio index {} out of range 0-{}", i, config.ngpio))?
                .name = Some(name.clone());
        }

        Ok(Self { config, uart_ports, gpio_lines, gpio_ports, i2c_port })
    }
}

impl Component for QemuComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, _status| {
            // Create uart port sockets
            let uart_ports: Vec<UartPort> = (0..self.config.nuart).map(|i| {
                self.config.uart_sockets.get(&i.to_string()).map(PathBuf::from).unwrap_or_else(||
                    std::env::temp_dir().join(format!("vuart{}-{}.sock", i, uuid::Uuid::new_v4()))
                )
            }).map(UartPort::new).collect();

            // Start uart backend
            let uart_backend_handle = start_uart_backend(uart_ports.clone(), logger.clone());

            // Create gpio socket
            let gpio_socket = self.config.gpio_socket.as_ref().map(PathBuf::from).unwrap_or_else(||
                std::env::temp_dir().join(format!("vgpio-{}.sock", uuid::Uuid::new_v4()))
            );

            // Start gpio backend
            let gpio_backend_handle = start_gpio_backend(&gpio_socket, self.gpio_lines, logger.clone());

            // Create i2c socket
            let i2c_socket = self.config.i2c_socket.as_ref().map(PathBuf::from).unwrap_or_else(||
                std::env::temp_dir().join(format!("vi2c-{}.sock", uuid::Uuid::new_v4()))
            );

            // Start i2c backend
            let i2c_backend_handle = start_i2c_backend(&i2c_socket, Duration::from_millis(self.config.i2c_timeout_ms), logger.clone());

            // Configure qemu instance
            let mut qemu_config = QemuConfig::new(self.config.arch, self.config.mem_mb);
            for option in self.config.options {
                let opt_arg = format!("-{}", option.option);
                match option.value {
                    Some(value) => qemu_config.add_option(opt_arg, value),
                    None => qemu_config.add_flag(opt_arg),
                }
            }

            // Set uart backend sockets
            for uart_port in uart_ports {
                qemu_config.add_uart_socket(uart_port.socket);
            }

            // Set gpio backend socket
            qemu_config.set_gpio_backend_socket(gpio_socket);

            // Set i2c backend socket
            qemu_config.set_i2c_backend_socket(i2c_socket);

            // Start qemu instance
            let qemu_stdout_logger = logger.clone();
            let qemu_stderr_logger = logger.clone();
            let qemu_instance = QemuInstance::spawn(
                qemu_config,
                move |line| { let _ = qemu_stdout_logger.info(format!("Qemu stdout: {}", line)); }, 
                move |line| { let _ = qemu_stderr_logger.error(format!("Qemu stderr: {}", line)); }, 
            )?;
            logger.info(format!("Spawned QEMU instance: {}", qemu_instance.cmd_line()))?;

            let mut dispatcher = MessageDispatcher::<QemuComponentState>::new();

            // Register command receiver handler
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    ComponentHandleCommand::Stop => {
                        logger.info(format!("Stopping Qemu instance"))?;
                        state.qemu_instance.quit()?;
                        state.qemu_instance.wait()?;
                        logger.info(format!("Qemu instance stopped"))?;
                        return Ok(HandlerStatus::Stop)
                    },
                    ComponentHandleCommand::StartStatusUpdates => state.update_status = true,
                    ComponentHandleCommand::StopStatusUpdates => state.update_status = false,
                    ComponentHandleCommand::SimulatorCommand { command, args } => {
                        match command.as_str() {
                            "qmp_command" => handle_qmp_command(state, args, &event_sender, &logger)?,
                            unknown => logger.error(format!("Unknown command: {}", unknown))?,
                        }
                    }
                }

                Ok(HandlerStatus::Ok)
            }));

            // Register uart backend requests handler
            dispatcher.register(uart_backend_handle.data_rx.clone(), Box::new(|trx, _state| {
                if let Some(port) = self.uart_ports.get(trx.port) {
                    port.send(trx.data);
                } else {
                    logger.error(format!("UART port out of range: {}", trx.port))?;
                }

                Ok(HandlerStatus::Ok)
            }));

            // Register uart port handlers
            for (i, uart_port) in self.uart_ports.iter().enumerate() {
                let uart_backend_data_tx = uart_backend_handle.data_tx.clone();
                dispatcher.register_system_port(uart_port, Box::new(move |msg, _state| {
                    uart_backend_data_tx.send(UartTransaction::new(i, msg))?;

                    Ok(HandlerStatus::Ok)
                }));
            }

            // Register gpio backend requests handler
            dispatcher.register(gpio_backend_handle.events_rx, Box::new(|req, _state| {
                let (line, value) = match req {
                    GpioEvent::ValueLow(line) => (line, false),
                    GpioEvent::ValueHigh(line) => (line, true),
                };

                if let Some(port) = self.gpio_ports.get(line as usize) {
                    port.send(value);
                } else {
                    logger.error(format!("GPIO line out of range: {}", line))?;
                }

                Ok(HandlerStatus::Ok)
            }));

            // Register gpio port handlers
            for (i, gpio_port) in self.gpio_ports.iter().enumerate() {
                let gpio_backend_events_tx = &gpio_backend_handle.events_tx;
                dispatcher.register_system_port(gpio_port, Box::new(move |msg, _state| {
                    let gpio_value_high = msg;

                    let event = if gpio_value_high {
                        GpioEvent::ValueHigh(i as u16)
                    } else {
                        GpioEvent::ValueLow(i as u16)
                    };

                    gpio_backend_events_tx.send(event)?;

                    Ok(HandlerStatus::Ok)
                }));
            }

            // Register i2c backend requests handler
            dispatcher.register(i2c_backend_handle.request_rx, Box::new(|req, _state| {
                self.i2c_port.send(req);

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c port handler
            dispatcher.register_system_port(&self.i2c_port, Box::new(|resp, _state| {
                i2c_backend_handle.response_tx.send(resp)?;

                Ok(HandlerStatus::Ok)
            }));

            // Start dispatcher
            let state = QemuComponentState::new(false, qemu_instance);
            dispatcher.start_dispatch(state)?;

            // Stop i2c backend
            uart_backend_handle.stop_and_join();

            // Stop gpio backend
            let _ = gpio_backend_handle.join_handle.join();

            // Stop i2c backend
            let _ = i2c_backend_handle.join_handle.join();

            Ok(())
        });

        Ok(handle)
    }

    fn from_params(params: ComponentParams) -> Result<Self> where Self: Sized {
        // Parse config parameters
        let config = serde_json::from_value::<QemuComponentConfig>(params.config().clone())?;

        let mut uart_ports = vec![SystemPort::default(); config.nuart as usize];
        let mut gpio_ports = vec![SystemPort::default(); config.ngpio as usize];
        let mut i2c_port = SystemPort::default();

        // Get system ports from params
        for (key, value) in &params.system_ports {
            match key.as_str() {
                "i2c" => {
                    i2c_port = value.as_i2c()?.clone();
                }

                k if k.starts_with("uart") => {
                    let idx: usize = k["uart".len()..]
                        .parse()
                        .map_err(|_| anyhow!("Unknown system port {} for QemuComponent", k))?;

                    let uart_port = uart_ports
                        .get_mut(idx)
                        .ok_or_else(|| anyhow!("UART port {} out of range 0-{}", k, config.nuart))?;

                    *uart_port = value.as_bytes()?.clone();
                }

                k if k.starts_with("gpio") => {
                    let idx: usize = k["gpio".len()..]
                        .parse()
                        .map_err(|_| anyhow!("Unknown system port {} for QemuComponent", k))?;

                    let gpio_port = gpio_ports
                        .get_mut(idx)
                        .ok_or_else(|| anyhow!("Gpio port {} out of range 0-{}", k, config.ngpio))?;

                    *gpio_port = value.as_bool()?.clone();
                }

                _ => {
                    bail!("Unknown system port {} for QemuComponent", key);
                }
            }
        }


        // Return the fully initialized component
        QemuComponent::new(config, uart_ports, gpio_ports, i2c_port)
    }
}

fn handle_qmp_command(
    state: &mut QemuComponentState,
    args: serde_json::Value,
    event_sender: &Sender<ComponentEvent>,
    logger: &Logger,
) -> Result<()> {
    #[derive(Deserialize)]
    struct QmpCommand {
        execute: String,
        #[serde(default)]
        arguments: Option<serde_json::Value>,
    }

    let qmp_command: QmpCommand = match serde_json::from_value(args) {
        Ok(cmd) => cmd,
        Err(e) => {
            logger.error(format!("Invalid qmp_command arguments: {}", e))?;
            return Ok(());
        }
    };

    let response = match state
        .qemu_instance
        .qmp_command(&qmp_command.execute, qmp_command.arguments)
    {
        Ok(r) => r,
        Err(e) => {
            logger.error(format!("Qmp command failed: {}", e))?;
            return Ok(());
        }
    };

    event_sender.send(ComponentEvent::sim_event("qmp_response", response))?;
    Ok(())
}
