use rand::rngs::StdRng;
use serde_json::Value;

use crate::ports::{
    I2cRequestOp, I2cResponseOp, I2cTransaction,
};

/// Result of applying a fuzz strategy to a message.
#[derive(Debug)]
pub enum MutationResult {
    /// The message passed through unchanged.
    Passed,
    /// The message was mutated. Contains a description of the mutation.
    Mutated(String),
    /// The message was dropped.
    Dropped,
}

/// A composable fuzzing strategy that can be applied to messages in the pipeline.
///
/// Strategies are applied sequentially: each strategy receives the (possibly already mutated)
/// message from the previous strategy. If any strategy returns `Dropped`, the pipeline
/// short-circuits.
///
/// The default implementations of `apply_bytes` and `apply_i2c` return `Passed` (no-op).
/// Concrete strategies override the methods relevant to the message types they handle.
pub trait FuzzStrategy: Send {
    /// Human-readable name for this strategy.
    fn name(&self) -> &str;

    /// Whether this strategy is currently enabled.
    fn enabled(&self) -> bool;

    /// Enable or disable this strategy.
    fn set_enabled(&mut self, enabled: bool);

    /// Apply the strategy to raw byte data (used for BytesMessage).
    fn apply_bytes(&mut self, _data: &mut Vec<u8>, _rng: &mut StdRng) -> MutationResult {
        MutationResult::Passed
    }

    /// Apply the strategy to an I2C transaction (used for I2cMessage).
    fn apply_i2c(&mut self, _msg: &mut I2cTransaction, _rng: &mut StdRng) -> MutationResult {
        MutationResult::Passed
    }

    /// Whether this strategy can drop messages (used for generic passthrough on non-byte types).
    fn can_drop(&self) -> bool {
        false
    }

    /// Decide whether to drop a message (only called for generic types like Bool/Int/Float/Text).
    fn should_drop(&mut self, _rng: &mut StdRng) -> bool {
        false
    }
}

/// Extract mutable references to payload bytes inside an I2C transaction.
/// Returns `None` if the transaction variant has no payload bytes (e.g., Read request, Write response).
pub fn i2c_payload_bytes_mut(msg: &mut I2cTransaction) -> Option<&mut Vec<u8>> {
    match msg {
        I2cTransaction::Request(req) => match &mut req.op {
            I2cRequestOp::Write(data) => Some(data),
            I2cRequestOp::Read(_) => None,
        },
        I2cTransaction::Response(resp) => match &mut resp.op {
            I2cResponseOp::Read(data) => Some(data),
            I2cResponseOp::Write => None,
        },
    }
}

/// Build a boxed strategy from a JSON config object with a "type" discriminator.
pub fn strategy_from_config(value: &Value) -> anyhow::Result<Box<dyn FuzzStrategy>> {
    let strategy_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Strategy config must have a \"type\" field"))?;

    match strategy_type {
        "drop" => Ok(Box::new(super::strategies::DropStrategy::from_config(value)?)),
        "bitflip" => Ok(Box::new(super::strategies::BitFlipStrategy::from_config(value)?)),
        "noise" => Ok(Box::new(super::strategies::NoiseStrategy::from_config(value)?)),
        "i2c_address" => Ok(Box::new(super::strategies::I2cAddressStrategy::from_config(value)?)),
        "i2c_op_flip" => Ok(Box::new(super::strategies::I2cOpFlipStrategy::from_config(value)?)),
        "i2c_read_length" => Ok(Box::new(super::strategies::I2cReadLengthStrategy::from_config(value)?)),
        "i2c_response" => Ok(Box::new(super::strategies::I2cResponseStrategy::from_config(value)?)),
        "i2c_payload" => Ok(Box::new(super::strategies::I2cPayloadStrategy::from_config(value)?)),
        other => Err(anyhow::anyhow!("Unknown strategy type: {}", other)),
    }
}
