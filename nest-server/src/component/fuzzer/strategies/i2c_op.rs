use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{I2cRequestOp, I2cResponseOp, I2cTransaction};

use super::super::strategy::{FuzzStrategy, MutationResult};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct I2cOpFlipConfig {
    flip_prob: f64,
}

impl Default for I2cOpFlipConfig {
    fn default() -> Self {
        Self { flip_prob: 0.1 }
    }
}

pub struct I2cOpFlipStrategy {
    enabled: bool,
    flip_prob: f64,
}

impl I2cOpFlipStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: I2cOpFlipConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            flip_prob: config.flip_prob,
        })
    }
}

impl FuzzStrategy for I2cOpFlipStrategy {
    fn name(&self) -> &str {
        "i2c_op_flip"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        if !rng.gen_bool(self.flip_prob.clamp(0.0, 1.0)) {
            return MutationResult::Passed;
        }

        match msg {
            I2cTransaction::Request(req) => {
                let desc = match &req.op {
                    I2cRequestOp::Write(data) => {
                        let len = data.len() as u32;
                        req.op = I2cRequestOp::Read(len);
                        format!("Write({} bytes) -> Read({})", len, len)
                    }
                    I2cRequestOp::Read(n) => {
                        let n = *n;
                        req.op = I2cRequestOp::Write(vec![0u8; n as usize]);
                        format!("Read({}) -> Write({} zero bytes)", n, n)
                    }
                };
                MutationResult::Mutated(format!("I2C op flip: {}", desc))
            }
            I2cTransaction::Response(resp) => {
                let desc = match &resp.op {
                    I2cResponseOp::Write => {
                        resp.op = I2cResponseOp::Read(vec![]);
                        "Write ACK -> empty Read".to_string()
                    }
                    I2cResponseOp::Read(data) => {
                        let len = data.len();
                        resp.op = I2cResponseOp::Write;
                        format!("Read({} bytes) -> Write ACK", len)
                    }
                };
                MutationResult::Mutated(format!("I2C op flip: {}", desc))
            }
        }
    }
}
