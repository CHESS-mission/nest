use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;

use crate::component::{
    Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
    MessageDispatcher,
};
use crate::ports::{BoolMessage, BytesMessage, SystemPort}; // ASSUMPTION: BytesMessage exists (Vec<u8>-like)

pub mod bridge;
pub mod error;
pub mod header;
pub mod status;

pub use bridge::{Bridge, BridgeError, I2cSpeed, UartParity, UartStopBits};
pub use header::{GpioChannel, I2cChannel, UartChannel};
pub use status::Status;

// Config

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct I2cConfig {
    speed: String, // "slow" | "fast"
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UartConfig {
    baudrate: u32,
    stop_bits: String, // "1" | "2"
    parity: String,    // "none" | "even" | "odd"
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HilConfig {
    #[serde(default)]
    i2c: HashMap<String, I2cConfig>,
    #[serde(default)] // was missing: an I2C-only setup would fail to parse
    uart: HashMap<String, UartConfig>,
}

// State

struct HilState {
    connected: bool,
}

// Component

struct GpioPort { channel: GpioChannel, port: SystemPort<BoolMessage> }
struct I2cPort  { channel: I2cChannel, speed: I2cSpeed, port: SystemPort<BytesMessage> }
struct UartPort {
    channel: UartChannel,
    baudrate: u32,
    stop_bits: UartStopBits,
    parity: UartParity,
    port: SystemPort<BytesMessage>,
}

#[derive(Default)]
pub struct HilComponent {
    gpio: Vec<GpioPort>,
    i2c: Vec<I2cPort>,
    uart: Vec<UartPort>,
}

impl Component for HilComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        // Connect and configure before spawning: a missing board fails startup immediately
        let bridge = Arc::new(Bridge::open()?);
        for i in &self.i2c {
            bridge.i2c_set_speed(i.channel, i.speed)?;
        }
        for u in &self.uart {
            bridge.uart_set_baudrate(u.channel, u.baudrate)?;
            bridge.uart_set_stop_bits(u.channel, u.stop_bits)?;
            bridge.uart_set_parity(u.channel, u.parity)?;
        }

        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, _status| {
            let logger = &logger; // lets `move` closures below copy a reference
            let rt = Arc::new(
                tokio::runtime::Builder::new_current_thread().enable_time().build()?,
            );
            let mut dispatcher = MessageDispatcher::<HilState>::new();

            // Basic commands (same as Mux)
            dispatcher.register(command_receiver, Box::new(|msg, _state| {
                match msg {
                    ComponentHandleCommand::Stop => {
                        logger.info("Stopping HIL component")?;
                        return Ok(HandlerStatus::Stop);
                    }
                    ComponentHandleCommand::StartStatusUpdates => {}
                    ComponentHandleCommand::StopStatusUpdates => {}
                    ComponentHandleCommand::SimulatorCommand { command, .. } => {
                        logger.error(format!("HIL does not support simulator command: {command}"))?;
                    }
                }
                Ok(HandlerStatus::Ok)
            }));

            // GPIO: simulation -> bridge
            for g in &self.gpio {
                let (bridge, channel) = (bridge.clone(), g.channel);
                dispatcher.register_system_port(&g.port, Box::new(move |msg, _state| {
                    if msg { bridge.gpio_set(channel)? } else { bridge.gpio_reset(channel)? }
                    Ok(HandlerStatus::Ok)
                }));
            }

            // UART: simulation -> bridge (handler), bridge -> simulation (listener)
            for u in &self.uart {
                let (bridge_tx, channel) = (bridge.clone(), u.channel);
                dispatcher.register_system_port(&u.port, Box::new(move |msg: BytesMessage, _state| {
                    bridge_tx.uart_transmit(channel, &msg)?;
                    Ok(HandlerStatus::Ok)
                }));

                let out = u.port.clone(); // runs on the bridge reader thread
                bridge.uart_set_receive_listener(channel, move |data| {
                    out.send(data.to_vec()); // ASSUMPTION: adjust conversion to your BytesMessage
                })?;
            }

            // I2C: request/response through the bridge
            for i in &self.i2c {
                let (bridge, rt, channel) = (bridge.clone(), rt.clone(), i.channel);
                let reply_port = i.port.clone();
                dispatcher.register_system_port(&i.port, Box::new(move |msg: BytesMessage, _state| {
                    match rt.block_on(i2c_transaction(&bridge, channel, &msg)) {
                        Ok(Some(reply)) => reply_port.send(reply),
                        Ok(None) => {}
                        // Transient bus errors (NACK, timeout) are logged, not fatal
                        Err(e) => logger.error(format!("I2C {channel:?}: {e}"))?,
                    }
                    Ok(HandlerStatus::Ok)
                }));
            }

            dispatcher.start_dispatch(HilState { connected: true })?;

            // Detach listeners so the bridge can shut down cleanly
            for u in &self.uart {
                bridge.uart_clear_receive_listener(u.channel);
            }
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
        let config = serde_json::from_value::<HilConfig>(params.config().clone())?;

        let names: Vec<String> = params.system_ports.keys().cloned().collect();
        for name in names {
            let port = params.system_ports.remove(&name).unwrap();

            if let Some(ch) = name.strip_prefix("gpio_") {
                let channel = parse_gpio(ch).ok_or_else(|| anyhow!("Unknown GPIO channel '{ch}'"))?;
                comp.gpio.push(GpioPort { channel, port: port.into_bool()? });
            } else if let Some(ch) = name.strip_prefix("i2c_") {
                let channel = parse_i2c(ch).ok_or_else(|| anyhow!("Unknown I2C channel '{ch}'"))?;
                let cfg = config.i2c.get(ch).ok_or_else(|| anyhow!("Missing i2c config for '{ch}'"))?;
                let speed = match cfg.speed.to_lowercase().as_str() {
                    "slow" => I2cSpeed::Slow,
                    "fast" => I2cSpeed::Fast,
                    s => bail!("Invalid I2C speed '{s}' (expected slow or fast)"),
                };
                comp.i2c.push(I2cPort { channel, speed, port: port.into_bytes()? }); // ASSUMPTION: into_bytes()
            } else if let Some(ch) = name.strip_prefix("uart_") {
                let channel = parse_uart(ch).ok_or_else(|| anyhow!("Unknown UART channel '{ch}'"))?;
                let cfg = config.uart.get(ch).ok_or_else(|| anyhow!("Missing uart config for '{ch}'"))?;
                let stop_bits = match cfg.stop_bits.as_str() {
                    "1" => UartStopBits::One,
                    "2" => UartStopBits::Two,
                    s => bail!("Invalid UART stop bits '{s}' (expected 1 or 2)"),
                };
                let parity = match cfg.parity.to_lowercase().as_str() {
                    "none" => UartParity::No,
                    "even" => UartParity::Even,
                    "odd" => UartParity::Odd,
                    s => bail!("Invalid UART parity '{s}' (expected none, even or odd)"),
                };
                comp.uart.push(UartPort {
                    channel, baudrate: cfg.baudrate, stop_bits, parity, port: port.into_bytes()?,
                });
            } else {
                bail!("Unknown system port {name} for HilComponent");
            }
        }

        // Config entries without a matching port are almost certainly typos
        for key in config.i2c.keys() {
            if parse_i2c(key).is_none() || !comp.i2c.iter().any(|p| Some(p.channel) == parse_i2c(key)) {
                bail!("i2c config '{key}' has no matching 'i2c_{key}' port");
            }
        }
        for key in config.uart.keys() {
            if parse_uart(key).is_none() || !comp.uart.iter().any(|p| Some(p.channel) == parse_uart(key)) {
                bail!("uart config '{key}' has no matching 'uart_{key}' port");
            }
        }

        Ok(comp)
    }
}

/// Message format: [addr, read_len, data...]
async fn i2c_transaction(bridge: &Bridge, ch: I2cChannel, msg: &[u8]) -> Result<Option<Vec<u8>>> {
    let [addr, read_len, data @ ..] = msg else {
        bail!("I2C message needs at least [addr, read_len]");
    };
    let (addr, read_len) = (*addr, u16::from(*read_len));
    Ok(match (data.is_empty(), read_len) {
        (true, 0) => bail!("empty I2C transaction"),
        (true, n) => Some(bridge.i2c_read(ch, addr, n).await?),
        (false, 0) => { bridge.i2c_write(ch, addr, data)?; None }
        (false, n) => Some(bridge.i2c_write_read(ch, addr, data, n).await?),
    })
}

fn parse_gpio(s: &str) -> Option<GpioChannel> {
    use GpioChannel::*;
    Some(match s {
        "adcs_boot" => AdcsBoot, "eps_hb_a" => EpsHbA, "eps_hb_b" => EpsHbB,
        "gnss_enable" => GnssEnable, "gnss_pwr_oc" => GnssPwrOc,
        "spad_cdone" => SpadCdone, "spad_pgood" => SpadPgood,
        "spad_reset" => SpadReset, "spad_bkpcfg" => SpadBkpcfg,
        _ => return None,
    })
}

fn parse_i2c(s: &str) -> Option<I2cChannel> {
    Some(match s { "main" => I2cChannel::Main, "eps" => I2cChannel::Eps, _ => return None })
}

fn parse_uart(s: &str) -> Option<UartChannel> {
    use UartChannel::*;
    Some(match s {
        "adcs" => Adcs, "uhf_a" => UhfA, "uhf_b" => UhfB, "gnss_a" => GnssA,
        "gnss_b" => GnssB, "adcs_rs422" => AdcsRs422, "spad_rs422" => SpadRs422,
        _ => return None,
    })
}
