use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::I2cTransaction;

use super::super::strategy::{i2c_payload_bytes_mut, FuzzStrategy, MutationResult};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct BitFlipConfig {
    flip_prob: f64,
    max_flips: usize,
}

impl Default for BitFlipConfig {
    fn default() -> Self {
        Self {
            flip_prob: 0.01,
            max_flips: 3,
        }
    }
}

pub struct BitFlipStrategy {
    enabled: bool,
    flip_prob: f64,
    max_flips: usize,
}

impl BitFlipStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: BitFlipConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            flip_prob: config.flip_prob,
            max_flips: config.max_flips,
        })
    }

    fn flip_bytes(&self, data: &mut [u8], rng: &mut StdRng) -> usize {
        let mut flips = 0;
        for byte in data.iter_mut() {
            if flips >= self.max_flips {
                break;
            }
            if rng.gen_bool(self.flip_prob.clamp(0.0, 1.0)) {
                let bit = rng.gen_range(0..8u8);
                *byte ^= 1 << bit;
                flips += 1;
            }
        }
        flips
    }
}

impl FuzzStrategy for BitFlipStrategy {
    fn name(&self) -> &str {
        "bitflip"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_bytes(&mut self, data: &mut Vec<u8>, rng: &mut StdRng) -> MutationResult {
        let flips = self.flip_bytes(data, rng);
        if flips > 0 {
            MutationResult::Mutated(format!("flipped {} bit(s)", flips))
        } else {
            MutationResult::Passed
        }
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        if let Some(payload) = i2c_payload_bytes_mut(msg) {
            let flips = self.flip_bytes(payload, rng);
            if flips > 0 {
                return MutationResult::Mutated(format!(
                    "flipped {} bit(s) in I2C payload",
                    flips
                ));
            }
        }
        MutationResult::Passed
    }
}
