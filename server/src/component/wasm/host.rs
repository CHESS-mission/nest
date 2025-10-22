use crossbeam_channel::Sender;
use wasmtime::component::*;

use crate::{
    component::{
        ComponentEvent, Logger,
        wasm::host::nest::plugin::types::{
            Bytes, I2cRequest, I2cRequestOp, I2cResponse, I2cResponseOp, I2cTransaction, Json,
        },
    },
    ports::{
        BoolMessage, BytesMessage, FloatMessage, I2cMessage, IntMessage, JsonMessage, TextMessage,
    },
};

bindgen!("nest-component" in "src/component/wasm/wit");

pub struct HostCtx {
    logger: Logger,
    sender: Sender<ComponentEvent>,
}

impl HostCtx {
    pub fn new(logger: Logger, sender: Sender<ComponentEvent>) -> Self {
        Self { logger, sender }
    }
}

impl nest::plugin::log::Host for HostCtx {
    fn info(&mut self, msg: String) -> () {
        let _ = self.logger.info(msg);
    }

    fn warning(&mut self, msg: String) -> () {
        let _ = self.logger.warning(msg);
    }

    fn error(&mut self, msg: String) -> () {
        let _ = self.logger.error(msg);
    }
}

pub trait IntoWit<T> {
    fn into_wit(self) -> T;
}

impl IntoWit<i32> for IntMessage {
    fn into_wit(self) -> i32 { self }
}

impl IntoWit<f32> for FloatMessage {
    fn into_wit(self) -> f32 { self }
}

impl IntoWit<bool> for BoolMessage {
    fn into_wit(self) -> bool { self }
}

impl IntoWit<Bytes> for BytesMessage {
    fn into_wit(self) -> Bytes { self }
}

impl IntoWit<String> for TextMessage {
    fn into_wit(self) -> String { self }
}

impl IntoWit<Json> for JsonMessage {
    fn into_wit(self) -> Json { self.to_string() }
}

impl IntoWit<I2cTransaction> for I2cMessage {
    fn into_wit(self) -> I2cTransaction {
        match self {
            I2cMessage::Request(i2c_request) => {
                I2cTransaction::Request(I2cRequest {
                    addr: i2c_request.addr,
                    op: match i2c_request.op {
                        crate::ports::I2cRequestOp::Write(buf) => I2cRequestOp::Write(buf),
                        crate::ports::I2cRequestOp::Read(len) => I2cRequestOp::Read(len),
                    },
                })
            }
            I2cMessage::Response(i2c_response) => {
                I2cTransaction::Response(I2cResponse {
                    op: match i2c_response.op {
                        crate::ports::I2cResponseOp::Write => I2cResponseOp::Write,
                        crate::ports::I2cResponseOp::Read(buf) => I2cResponseOp::Read(buf),
                    },
                })
            }
        }
    }
}

pub trait FromWit<T> {
    fn from_wit(wit_val: T) -> Self;
}

impl FromWit<i32> for IntMessage {
    fn from_wit(wit_val: i32) -> Self { wit_val }
}

impl FromWit<f32> for FloatMessage {
    fn from_wit(wit_val: f32) -> Self { wit_val }
}

impl FromWit<bool> for BoolMessage {
    fn from_wit(wit_val: bool) -> Self { wit_val }
}

impl FromWit<Bytes> for BytesMessage {
    fn from_wit(wit_val: Vec<u8>) -> Self { wit_val }
}

impl FromWit<String> for TextMessage {
    fn from_wit(wit_val: String) -> Self { wit_val }
}

impl FromWit<Json> for JsonMessage {
    fn from_wit(wit_val: Json) -> Self { serde_json::from_str(&wit_val).unwrap() } // Maybe remove JSON completely...
}

impl FromWit<I2cTransaction> for I2cMessage {
    fn from_wit(wit_val: I2cTransaction) -> Self {
        match wit_val {
            I2cTransaction::Request(i2c_request) => {
                crate::ports::I2cTransaction::Request(crate::ports::I2cRequest {
                    addr: i2c_request.addr,
                    op: match i2c_request.op {
                        I2cRequestOp::Write(buf) => crate::ports::I2cRequestOp::Write(buf),
                        I2cRequestOp::Read(len) => crate::ports::I2cRequestOp::Read(len),
                    },
                })
            }
            I2cTransaction::Response(i2c_response) => {
                crate::ports::I2cTransaction::Response(crate::ports::I2cResponse {
                    op: match i2c_response.op {
                        I2cResponseOp::Write => crate::ports::I2cResponseOp::Write,
                        I2cResponseOp::Read(buf) => crate::ports::I2cResponseOp::Read(buf),
                    },
                })
            }
        }
    }
}
