// SystemPort

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::{
    any_env_var_enum, any_message_enum,
    environment::{AnySharedEnvironmentVar, Lerp, SharedEnvironmentVar},
};

/// [SystemPort] builder for creating message channels and setting up connections
/// with other ports.
#[derive(Clone, Debug)]
pub struct SystemPortBuilder<M: Clone + Send + Sync + 'static> {
    rx: Receiver<M>,
    local_tx: Sender<M>,
    connections: Vec<Sender<M>>,
}

impl<M: Clone + Send + Sync + 'static> SystemPortBuilder<M> {
    /// Create a new [SystemPortBuilder] of given queue capacity.
    pub fn new(queue_capacity: PortQueueCapacity) -> Self {
        let (local_tx, rx) = match queue_capacity {
            PortQueueCapacity::Unbounded => unbounded(),
            PortQueueCapacity::Bounded(cap) => bounded(cap),
        };
        Self { rx, local_tx, connections: Vec::new() }
    }

    /// Connect the port to another [SystemPortBuilder].
    ///
    /// Note: this only means that messages sent through this port will be received
    /// by the other port, but not the other way around. For bi-directional connections,
    /// the other port should also connect to this port.
    pub fn connect_to(&mut self, other: &SystemPortBuilder<M>) {
        self.connections.push(other.local_tx.clone());
    }

    /// Build a new [SystemPort] from the [SystemPortBuilder].
    pub fn build(self) -> SystemPort<M> {
        SystemPort {
            rx: self.rx,
            connections: self.connections,
        }
    }
}

impl<M: Clone + Send + Sync + 'static> Default for SystemPortBuilder<M> {
    fn default() -> Self {
        Self::new(PortQueueCapacity::Unbounded)
    }
}

any_message_enum!(AnySystemPortBuilder, SystemPortBuilder);

impl AnySystemPortBuilder {
    /// Name of the message type the port carries, as used in topology files.
    pub fn type_name(&self) -> &'static str {
        match self {
            AnySystemPortBuilder::Int(_) => "int",
            AnySystemPortBuilder::Float(_) => "float",
            AnySystemPortBuilder::Bool(_) => "bool",
            AnySystemPortBuilder::Bytes(_) => "bytes",
            AnySystemPortBuilder::Text(_) => "text",
            AnySystemPortBuilder::I2c(_) => "i2c",
        }
    }

    /// Connect the port to another [AnySystemPortBuilder]. Fails if the underlying ports are of
    /// different types.
    ///
    /// Note: this only means that messages sent through this port will be received
    /// by the other port, but not the other way around. For bi-directional connections,
    /// the other port should also connect to this port.
    pub fn connect_to(&mut self, other: &AnySystemPortBuilder) -> Result<()> {
        match self {
            AnySystemPortBuilder::Int(port) => port.connect_to(other.as_int()?),
            AnySystemPortBuilder::Float(port) => port.connect_to(other.as_float()?),
            AnySystemPortBuilder::Bool(port) => port.connect_to(other.as_bool()?),
            AnySystemPortBuilder::Bytes(port) => port.connect_to(other.as_bytes()?),
            AnySystemPortBuilder::Text(port) => port.connect_to(other.as_text()?),
            AnySystemPortBuilder::I2c(port) => port.connect_to(other.as_i2c()?),
        }
        Ok(())
    }

    /// Build a new [AnySystemPort] from the [AnySystemPortBuilder].
    pub fn build(self) -> AnySystemPort {
        match self {
            AnySystemPortBuilder::Int(port) => AnySystemPort::Int(port.build()),
            AnySystemPortBuilder::Float(port) => AnySystemPort::Float(port.build()),
            AnySystemPortBuilder::Bool(port) => AnySystemPort::Bool(port.build()),
            AnySystemPortBuilder::Bytes(port) => AnySystemPort::Bytes(port.build()),
            AnySystemPortBuilder::Text(port) => AnySystemPort::Text(port.build()),
            AnySystemPortBuilder::I2c(port) => AnySystemPort::I2c(port.build()),
        }
    }
}

/// Simple wrapper around crossbeam_channel receiver (one) and senders (many) with a Message type.
#[derive(Clone, Debug)]
pub struct SystemPort<M: Clone + Send + Sync + 'static> {
    rx: Receiver<M>,
    connections: Vec<Sender<M>>,
}

impl<M: Clone + Send + Sync + 'static> SystemPort<M> {
    /// Create a new [SystemPort].
    pub fn new(rx: Receiver<M>, connections: Vec<Sender<M>>) -> Self {
        Self { rx, connections }
    }

    /// Send a message through the port.
    pub fn send(&self, msg: M) {
        // Avoid unnecessary clones of msg
        // Use try_send to ignore channel full (avoid deadlocks) or disconnected
        match self.connections.len() {
            0 => {},
            1 => {
                let _ = self.connections[0].try_send(msg);
            },
            n => {
                for tx in &self.connections[..n-1] {
                    let _ = tx.try_send(msg.clone());
                }
                let _ = self.connections[n-1].try_send(msg);
            },
        }
    }

    /// Return a reference to the port's receiver
    pub fn receiver(&self) -> &Receiver<M> {
        &self.rx
    }
}

impl<M: Clone + Send + Sync + 'static> Default for SystemPort<M> {
    fn default() -> Self {
        SystemPortBuilder::default().build()
    }
}

any_message_enum!(AnySystemPort, SystemPort);

/// Message queue capacity. Can be bounded or unbounded.
#[derive(Clone, Debug)]
pub enum PortQueueCapacity {
    /// Unbounded queue capacity.
    Unbounded,
    /// Bounded queue capacity by a maximum number of queued messages.
    ///
    /// Send operations to a bounded channel block until space is available.
    Bounded(usize),
}

// Environment port

/// Simple read-only wrapper around a [SharedEnvironmentVar].
#[derive(Clone, Debug)]
pub struct EnvironmentPort<V: Lerp + Default + Clone + Send + Sync + 'static> {
    env_var: SharedEnvironmentVar<V>,
}

impl<V: Lerp + Default + Clone + Send + Sync + 'static> EnvironmentPort<V> {
    /// Create a new [EnvironmentPort].
    pub fn new(env_var: SharedEnvironmentVar<V>) -> Self {
        Self { env_var }
    }

    /// Read the underlying value by **cloning** it.
    /// Holds a read lock only for the time it takes to clone the value.
    pub fn value(&self) -> V {
        self.env_var.read().unwrap().get()
    }
}

any_env_var_enum!(AnyEnvironmentPort, EnvironmentPort);

impl From<AnySharedEnvironmentVar> for AnyEnvironmentPort {
    fn from(value: AnySharedEnvironmentVar) -> Self {
        match value {
            AnySharedEnvironmentVar::Int(env_var) => AnyEnvironmentPort::Int(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Float(env_var) => AnyEnvironmentPort::Float(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Bool(env_var) => AnyEnvironmentPort::Bool(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Vec2(env_var) => AnyEnvironmentPort::Vec2(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Vec3(env_var) => AnyEnvironmentPort::Vec3(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Quaternion(env_var) => AnyEnvironmentPort::Quaternion(EnvironmentPort::new(env_var)),
            AnySharedEnvironmentVar::Tensor(env_var) => AnyEnvironmentPort::Tensor(EnvironmentPort::new(env_var)),
        }
    }
}

// Message

/// Message sent between components. 
///
/// Available message types are:
///
/// - [IntMessage]
/// - [FloatMessage]
/// - [BoolMessage]
/// - [BytesMessage]
/// - [TextMessage]
/// - [I2cMessage]
pub type IntMessage = i32;
pub type FloatMessage = f32;
pub type BoolMessage = bool;
pub type BytesMessage = Vec<u8>;
pub type TextMessage = String;
pub type I2cMessage = I2cTransaction;

/// Represents an I2C transaction (request or response).
#[derive(Debug, Clone)]
pub enum I2cTransaction {
    /// An I2c request sent from the master to a device.
    Request(I2cRequest),
    /// A response sent from the device back to the master.
    Response(I2cResponse),
}

/// An I2c request.
#[derive(Debug, Clone)]
pub struct I2cRequest {
    /// The 7-bit I2C device address.
    pub addr: u16,
    /// The requested operation (read or write).
    pub op: I2cRequestOp,
}

impl I2cRequest {
    pub fn new(addr: u16, op: I2cRequestOp) -> Self {
        Self { addr, op }
    }
}

/// An I2C request operation.
#[derive(Debug, Clone)]
pub enum I2cRequestOp {
    /// A write operation, containing the data to be written.
    Write(Vec<u8>),
    /// A read operation, indicating the number of bytes to read.
    Read(u32),
}

/// An I2c response.
#[derive(Debug, Clone)]
pub struct I2cResponse {
    /// The operation response (read or write).
    pub op: I2cResponseOp,
}

impl I2cResponse {
    pub fn new(op: I2cResponseOp) -> Self {
        Self { op }
    }
}

/// An I2C response operation.
#[derive(Debug, Clone)]
pub enum I2cResponseOp {
    /// A write operation response (simple ACK).
    Write,
    /// A read operation response, with the data that was read.
    Read(Vec<u8>),
}
