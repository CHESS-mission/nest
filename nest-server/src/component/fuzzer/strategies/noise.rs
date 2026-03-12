use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::I2cTransaction;

use super::super::strategy::{i2c_payload_bytes_mut, FuzzStrategy, MutationResult};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct NoiseConfig {
    noise_prob: f64,
    noise_min: u8,
    noise_max: u8,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            noise_prob: 0.05,
            noise_min: 0,
            noise_max: 255,
        }
    }
}

pub struct NoiseStrategy {
    enabled: bool,
    noise_prob: f64,
    noise_min: u8,
    noise_max: u8,
}

impl NoiseStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: NoiseConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            noise_prob: config.noise_prob,
            noise_min: config.noise_min,
            noise_max: config.noise_max,
        })
    }

    fn inject_noise(&self, data: &mut [u8], rng: &mut StdRng) -> usize {
        let mut replacements = 0;
        for byte in data.iter_mut() {
            if rng.gen_bool(self.noise_prob.clamp(0.0, 1.0)) {
                *byte = rng.gen_range(self.noise_min..=self.noise_max);
                replacements += 1;
            }
        }
        replacements
    }
}

impl FuzzStrategy for NoiseStrategy {
    fn name(&self) -> &str {
        "noise"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_bytes(&mut self, data: &mut Vec<u8>, rng: &mut StdRng) -> MutationResult {
        let n = self.inject_noise(data, rng);
        if n > 0 {
            MutationResult::Mutated(format!("replaced {} byte(s) with noise", n))
        } else {
            MutationResult::Passed
        }
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        if let Some(payload) = i2c_payload_bytes_mut(msg) {
            let n = self.inject_noise(payload, rng);
            if n > 0 {
                return MutationResult::Mutated(format!(
                    "replaced {} byte(s) with noise in I2C payload",
                    n
                ));
            }
        }
        MutationResult::Passed
    }
}
