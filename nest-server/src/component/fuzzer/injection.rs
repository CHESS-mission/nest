use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;

use crate::ports::{
    BytesMessage, I2cMessage, I2cRequest, I2cRequestOp, I2cTransaction,
};

/// Configuration for phantom message injection.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct InjectionConfig {
    pub enabled: bool,
    #[serde(default = "default_direction")]
    pub direction: String,
    pub interval_ms: u64,
    pub payload_len_min: usize,
    pub payload_len_max: usize,
}

fn default_direction() -> String {
    "comp_to_syst".to_string()
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            direction: default_direction(),
            interval_ms: 1000,
            payload_len_min: 1,
            payload_len_max: 32,
        }
    }
}

/// Generates random messages for phantom injection.
pub struct MessageGenerator;

impl MessageGenerator {
    /// Generate a random `BytesMessage` (Vec<u8>).
    pub fn random_bytes(rng: &mut StdRng, min_len: usize, max_len: usize) -> BytesMessage {
        let len = if min_len >= max_len {
            min_len
        } else {
            rng.gen_range(min_len..=max_len)
        };
        (0..len).map(|_| rng.r#gen()).collect()
    }

    /// Generate a random `I2cMessage` (I2cTransaction::Request with random address and payload).
    pub fn random_i2c(rng: &mut StdRng, min_len: usize, max_len: usize) -> I2cMessage {
        let addr = rng.gen_range(0x00..=0x7Fu16);
        let op = if rng.gen_bool(0.5) {
            let data = Self::random_bytes(rng, min_len, max_len);
            I2cRequestOp::Write(data)
        } else {
            let len = if min_len >= max_len {
                min_len as u32
            } else {
                rng.gen_range(min_len as u32..=max_len as u32)
            };
            I2cRequestOp::Read(len)
        };
        I2cTransaction::Request(I2cRequest::new(addr, op))
    }
}
