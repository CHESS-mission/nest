use crossbeam_channel::Sender;
use ndarray::IxDyn;
use wasmtime::component::*;

use crate::{
    component::{
        ComponentEvent, Logger,
        wasm::host::nest::plugin::types::{
            Bytes, I2cRequest, I2cRequestOp, I2cResponse, I2cResponseOp, I2cTransaction, Quaternion, Tensor, Vec2, Vec3
        },
    },
    ports::{
        BoolMessage, BytesMessage, FloatMessage, I2cMessage, IntMessage, TextMessage
    },
    environment::{
        QuaternionVar, TensorVar, Vec2Var, Vec3Var
    }
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

impl nest::plugin::types::Host for HostCtx {}
impl nest::plugin::errors::Host for HostCtx {}

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

impl NestComponentImports for HostCtx {
    fn emit(&mut self, event: String, value: String) -> () {
        if let Ok(value) = serde_json::from_str(&value) {
            let _ = self.sender.send(ComponentEvent::SimulationEvent { event, value });
        } else {
            let _ = self.sender.send(ComponentEvent::Error(format!("Received invalid JSON for event {}: {}", event, value)));
        }
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

impl IntoWit<Vec2> for Vec2Var {
    fn into_wit(self) -> Vec2 {
        Vec2 {
            x: self.x,
            y: self.y,
        }
    }
}

impl IntoWit<Vec3> for Vec3Var {
    fn into_wit(self) -> Vec3 {
        Vec3 {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

impl IntoWit<Quaternion> for QuaternionVar {
    fn into_wit(self) -> Quaternion {
        Quaternion {
            a: self.w,
            b: self.i,
            c: self.j,
            d: self.k,
        }
    }
}

impl IntoWit<Tensor> for TensorVar {
    fn into_wit(self) -> Tensor {
        Tensor {
            shape: self.shape().into_iter().map(|x| *x as u32).collect(),
            data: self.as_slice().unwrap().to_vec(),
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

impl FromWit<Vec2> for Vec2Var {
    fn from_wit(v: Vec2) -> Self {
        Vec2Var::new(v.x, v.y)
    }
}

impl FromWit<Vec3> for Vec3Var {
    fn from_wit(v: Vec3) -> Self {
        Vec3Var::new(v.x, v.y, v.z)
    }
}

impl FromWit<Quaternion> for QuaternionVar {
    fn from_wit(q: Quaternion) -> Self {
        QuaternionVar::from_quaternion(nalgebra::Quaternion::new(q.a, q.b, q.c, q.d))
    }
}

impl FromWit<Tensor> for TensorVar {
    fn from_wit(t: Tensor) -> Self {
        let shape: Vec<usize> = t.shape.into_iter().map(|x| x as usize).collect();
        TensorVar::from_shape_vec(IxDyn(&shape), t.data)
            .expect("invalid shape/data length in Tensor")
    }
}

