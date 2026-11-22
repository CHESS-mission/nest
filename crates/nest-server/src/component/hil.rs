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

pub mod bridge;
pub mod error;
pub mod header;
pub mod status;

pub use bridge::{Bridge, BridgeError, I2cSpeed, UartParity, UartStopBits};
pub use header::{GpioChannel, I2cChannel, UartChannel};
pub use status::Status;

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

impl Component for HilComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        unimplemented!();
    }

    fn from_params(mut params: ComponentParams) -> Result<Self>
    where
        Self: Sized,
    {
        let mut comp = Self::default();

        let config = serde_json::from_value::<HilConfig>(params.config().clone())?;

        unimplemented!();
    }
}
