use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::I2cTransaction;

use super::super::strategy::{i2c_payload_bytes_mut, FuzzStrategy, MutationResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PayloadMode {
    Random,
    Boundary,
    Truncate,
    Extend,
}

impl Default for PayloadMode {
    fn default() -> Self {
        Self::Random
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct I2cPayloadConfig {
    mode: PayloadMode,
    max_extend: usize,
}

impl Default for I2cPayloadConfig {
    fn default() -> Self {
        Self {
            mode: PayloadMode::Random,
            max_extend: 16,
        }
    }
}

pub struct I2cPayloadStrategy {
    enabled: bool,
    mode: PayloadMode,
    max_extend: usize,
}

impl I2cPayloadStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: I2cPayloadConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            mode: config.mode,
            max_extend: config.max_extend,
        })
    }
}

impl FuzzStrategy for I2cPayloadStrategy {
    fn name(&self) -> &str {
        "i2c_payload"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        let Some(payload) = i2c_payload_bytes_mut(msg) else {
            return MutationResult::Passed;
        };

        if payload.is_empty() {
            return MutationResult::Passed;
        }

        let desc = match self.mode {
            PayloadMode::Random => {
                let len = payload.len();
                for byte in payload.iter_mut() {
                    *byte = rng.r#gen();
                }
                format!("replaced {} byte(s) with random data", len)
            }
            PayloadMode::Boundary => {
                let len = payload.len();
                let boundary: u8 = *[0x00, 0xFF].choose(rng).unwrap_or(&0x00);
                for byte in payload.iter_mut() {
                    *byte = boundary;
                }
                format!("set {} byte(s) to 0x{:02X}", len, boundary)
            }
            PayloadMode::Truncate => {
                let original_len = payload.len();
                let new_len = rng.gen_range(0..original_len);
                payload.truncate(new_len);
                format!("truncated {} -> {} bytes", original_len, new_len)
            }
            PayloadMode::Extend => {
                let original_len = payload.len();
                let extra = rng.gen_range(1..=self.max_extend);
                for _ in 0..extra {
                    payload.push(rng.r#gen());
                }
                format!(
                    "extended {} -> {} bytes",
                    original_len,
                    original_len + extra
                )
            }
        };

        MutationResult::Mutated(format!("I2C payload: {}", desc))
    }
}

/// Extension trait to choose from a slice with an rng.
trait SliceChoose {
    type Item;
    fn choose(&self, rng: &mut StdRng) -> Option<&Self::Item>;
}

impl<T> SliceChoose for [T] {
    type Item = T;
    fn choose(&self, rng: &mut StdRng) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            Some(&self[rng.gen_range(0..self.len())])
        }
    }
}
