use serde::{Deserialize, Serialize};

/// An I2C operation, part of a transfer.
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub enum I2cOperation {
    /// A write operation, containing the data to be written.
    Write(Vec<u8>),
    /// A read operation, indicating the number of bytes to read.
    Read(usize),
}

/// A request sent from the [VhostUserI2cBackend] to the simulated device.
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub struct I2cRequest {
    /// The 7-bit I2C device address.
    pub addr: u16,
    /// The operation of this transaction.
    pub op: I2cOperation,
}

impl I2cRequest {
    pub fn is_read(&self) -> bool {
        matches!(self.op, I2cOperation::Read(_))
    }
}

/// A response sent from the simulated device back to the [VhostUserI2cBackend].
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub enum I2cResponse {
    /// A write response, acknowledging that the data has been written.
    Write,
    /// A read response, with the content read from the device.
    Read(Vec<u8>),
}
