use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{I2cTransaction};

use super::super::strategy::{FuzzStrategy, MutationResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AddressMode {
    Random,
    Sweep,
}

impl Default for AddressMode {
    fn default() -> Self {
        Self::Random
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct I2cAddressConfig {
    mode: AddressMode,
    range_min: u16,
    range_max: u16,
}

impl Default for I2cAddressConfig {
    fn default() -> Self {
        Self {
            mode: AddressMode::Random,
            range_min: 0x00,
            range_max: 0x7F,
        }
    }
}

pub struct I2cAddressStrategy {
    enabled: bool,
    mode: AddressMode,
    range_min: u16,
    range_max: u16,
    sweep_cursor: u16,
}

impl I2cAddressStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: I2cAddressConfig = serde_json::from_value(value.clone())?;
        let cursor = config.range_min;
        Ok(Self {
            enabled: true,
            mode: config.mode,
            range_min: config.range_min,
            range_max: config.range_max,
            sweep_cursor: cursor,
        })
    }
}

impl FuzzStrategy for I2cAddressStrategy {
    fn name(&self) -> &str {
        "i2c_address"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        let I2cTransaction::Request(req) = msg else {
            return MutationResult::Passed;
        };

        let original = req.addr;
        let new_addr = match self.mode {
            AddressMode::Random => rng.gen_range(self.range_min..=self.range_max),
            AddressMode::Sweep => {
                let addr = self.sweep_cursor;
                self.sweep_cursor = if self.sweep_cursor >= self.range_max {
                    self.range_min
                } else {
                    self.sweep_cursor + 1
                };
                addr
            }
        };

        req.addr = new_addr;
        MutationResult::Mutated(format!(
            "I2C addr 0x{:02X} -> 0x{:02X}",
            original, new_addr
        ))
    }
}
