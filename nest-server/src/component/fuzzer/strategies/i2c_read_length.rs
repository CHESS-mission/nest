use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{I2cRequestOp, I2cTransaction};

use super::super::strategy::{FuzzStrategy, MutationResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReadLengthMode {
    Random,
    Boundary,
}

impl Default for ReadLengthMode {
    fn default() -> Self {
        Self::Random
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct I2cReadLengthConfig {
    mode: ReadLengthMode,
    max_len: u32,
}

impl Default for I2cReadLengthConfig {
    fn default() -> Self {
        Self {
            mode: ReadLengthMode::Random,
            max_len: 256,
        }
    }
}

pub struct I2cReadLengthStrategy {
    enabled: bool,
    mode: ReadLengthMode,
    max_len: u32,
}

impl I2cReadLengthStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: I2cReadLengthConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            mode: config.mode,
            max_len: config.max_len,
        })
    }
}

impl FuzzStrategy for I2cReadLengthStrategy {
    fn name(&self) -> &str {
        "i2c_read_length"
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
        let I2cRequestOp::Read(ref mut n) = req.op else {
            return MutationResult::Passed;
        };

        let original = *n;
        let new_len = match self.mode {
            ReadLengthMode::Random => rng.gen_range(0..=self.max_len),
            ReadLengthMode::Boundary => {
                let candidates: Vec<u32> = [
                    0,
                    1,
                    original.saturating_sub(1),
                    original.saturating_add(1),
                    self.max_len,
                    u32::MAX,
                ]
                .into_iter()
                .collect();
                candidates[rng.gen_range(0..candidates.len())]
            }
        };

        *n = new_len;
        MutationResult::Mutated(format!("I2C read length {} -> {}", original, new_len))
    }
}
