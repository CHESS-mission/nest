use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::I2cTransaction;

use super::super::strategy::{FuzzStrategy, MutationResult};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct DropConfig {
    drop_prob: f64,
}

impl Default for DropConfig {
    fn default() -> Self {
        Self { drop_prob: 0.1 }
    }
}

pub struct DropStrategy {
    enabled: bool,
    drop_prob: f64,
}

impl DropStrategy {
    pub fn new(drop_prob: f64) -> Self {
        Self {
            enabled: true,
            drop_prob,
        }
    }

    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: DropConfig = serde_json::from_value(value.clone())?;
        Ok(Self::new(config.drop_prob))
    }

    fn decide_drop(&mut self, rng: &mut StdRng) -> MutationResult {
        if self.drop_prob > 0.0 && rng.gen_bool(self.drop_prob.clamp(0.0, 1.0)) {
            MutationResult::Dropped
        } else {
            MutationResult::Passed
        }
    }
}

impl FuzzStrategy for DropStrategy {
    fn name(&self) -> &str {
        "drop"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_bytes(&mut self, _data: &mut Vec<u8>, rng: &mut StdRng) -> MutationResult {
        self.decide_drop(rng)
    }

    fn apply_i2c(&mut self, _msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        self.decide_drop(rng)
    }

    fn can_drop(&self) -> bool {
        true
    }

    fn should_drop(&mut self, rng: &mut StdRng) -> bool {
        self.drop_prob > 0.0 && rng.gen_bool(self.drop_prob.clamp(0.0, 1.0))
    }
}
