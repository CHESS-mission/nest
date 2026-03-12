use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{I2cResponseOp, I2cTransaction};

use super::super::strategy::{FuzzStrategy, MutationResult};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct I2cResponseConfig {
    corrupt_prob: f64,
}

impl Default for I2cResponseConfig {
    fn default() -> Self {
        Self { corrupt_prob: 0.1 }
    }
}

pub struct I2cResponseStrategy {
    enabled: bool,
    corrupt_prob: f64,
}

impl I2cResponseStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: I2cResponseConfig = serde_json::from_value(value.clone())?;
        Ok(Self {
            enabled: true,
            corrupt_prob: config.corrupt_prob,
        })
    }
}

impl FuzzStrategy for I2cResponseStrategy {
    fn name(&self) -> &str {
        "i2c_response"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_i2c(&mut self, msg: &mut I2cTransaction, rng: &mut StdRng) -> MutationResult {
        let I2cTransaction::Response(resp) = msg else {
            return MutationResult::Passed;
        };

        if !rng.gen_bool(self.corrupt_prob.clamp(0.0, 1.0)) {
            return MutationResult::Passed;
        }

        let desc = match &resp.op {
            I2cResponseOp::Write => {
                resp.op = I2cResponseOp::Read(vec![]);
                "Write ACK -> empty Read response".to_string()
            }
            I2cResponseOp::Read(data) => {
                if rng.gen_bool(0.5) {
                    // Truncate data
                    let new_len = if data.is_empty() {
                        0
                    } else {
                        rng.gen_range(0..data.len())
                    };
                    let truncated = data[..new_len].to_vec();
                    let desc = format!(
                        "Read response truncated {} -> {} bytes",
                        data.len(),
                        new_len
                    );
                    resp.op = I2cResponseOp::Read(truncated);
                    desc
                } else {
                    // Flip to Write ACK (drop response data)
                    let desc = format!(
                        "Read({} bytes) response -> Write ACK",
                        data.len()
                    );
                    resp.op = I2cResponseOp::Write;
                    desc
                }
            }
        };

        MutationResult::Mutated(format!("I2C response corrupt: {}", desc))
    }
}
