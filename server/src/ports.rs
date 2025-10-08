// SystemPort

use std::sync::{Arc, RwLock, RwLockReadGuard};

use anyhow::{Result, anyhow};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use serde_json::Value;

use crate::any_message_enum;

/// [SystemPort] builder for creating message channels and setting up connections
/// with other ports.
#[derive(Clone, Debug)]
pub struct SystemPortBuilder<M: Message> {
    rx: Receiver<M>,
    local_tx: Sender<M>,
    connections: Vec<Sender<M>>,
}

impl<M: Message> SystemPortBuilder<M> {
    /// Create a new [SystemPortBuilder] of given queue capacity.
    pub fn new(queue_capacity: PortQueueCapacity) -> Self {
        let (local_tx, rx) = match queue_capacity {
            PortQueueCapacity::Unbounded => unbounded(),
            PortQueueCapacity::Bounded(cap) => bounded(cap),
        };
        Self {
            rx,
            local_tx,
            connections: Vec::new(),
        }
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

impl<M: Message> Default for SystemPortBuilder<M> {
    fn default() -> Self {
        Self::new(PortQueueCapacity::Unbounded)
    }
}

any_message_enum!(AnySystemPortBuilder, SystemPortBuilder);

impl AnySystemPortBuilder {
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
            AnySystemPortBuilder::Json(port) => port.connect_to(other.as_json()?),
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
            AnySystemPortBuilder::Json(port) => AnySystemPort::Json(port.build()),
        }
    }
}

/// Simple wrapper around crossbeam_channel receiver (one) and senders (many) with a Message type.
#[derive(Clone, Debug)]
pub struct SystemPort<M: Message> {
    rx: Receiver<M>,
    connections: Vec<Sender<M>>,
}

impl<M: Message> SystemPort<M> {
    /// Create a new [SystemPort].
    pub fn new(rx: Receiver<M>, connections: Vec<Sender<M>>) -> Self {
        Self { rx, connections }
    }

    /// Send a message through the port.
    pub fn send(&self, msg: M) {
        // Avoid unnecessary clones of msg
        // Use try_send to ignore channel full (avoid deadlocks) or disconnected
        match self.connections.len() {
            0 => {}
            1 => {
                let _ = self.connections[0].try_send(msg);
            }
            n => {
                for tx in &self.connections[..n - 1] {
                    let _ = tx.try_send(msg.clone());
                }
                let _ = self.connections[n - 1].try_send(msg);
            }
        }
    }

    /// Return a reference to the port's receiver
    pub fn receiver(&self) -> &Receiver<M> {
        &self.rx
    }
}

impl<M: Message> Default for SystemPort<M> {
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

/// Simple read-only wrapper around an [EnvironmentVar].
#[derive(Clone, Debug)]
pub struct EnvironmentPort<M: Message> {
    env_var: EnvironmentVar<M>,
}

impl<M: Message> EnvironmentPort<M> {
    /// Create a new [EnvironmentPort].
    pub fn new(env_var: EnvironmentVar<M>) -> Self {
        Self { env_var }
    }

    /// Read the underlying value by **cloning** it.
    /// Holds a read lock only for the time it takes to clone the value.
    pub fn value(&self) -> M {
        self.env_var.read().unwrap().clone()
    }

    /// Read the underlying value by **reference**.
    /// Holds a read lock for as long as the reference is not dropped.
    /// Returns a [`RwLockReadGuard`] so the caller can access it directly.
    pub fn read(&self) -> RwLockReadGuard<'_, M> {
        self.env_var.read().unwrap()
    }
}

any_message_enum!(AnyEnvironmentPort, EnvironmentPort);

impl From<AnyEnvironmentVar> for AnyEnvironmentPort {
    fn from(value: AnyEnvironmentVar) -> Self {
        match value {
            AnyEnvironmentVar::Int(env_var) => {
                AnyEnvironmentPort::Int(EnvironmentPort::new(env_var))
            }
            AnyEnvironmentVar::Float(env_var) => {
                AnyEnvironmentPort::Float(EnvironmentPort::new(env_var))
            }
            AnyEnvironmentVar::Bool(env_var) => {
                AnyEnvironmentPort::Bool(EnvironmentPort::new(env_var))
            }
            AnyEnvironmentVar::Bytes(env_var) => {
                AnyEnvironmentPort::Bytes(EnvironmentPort::new(env_var))
            }
            AnyEnvironmentVar::Text(env_var) => {
                AnyEnvironmentPort::Text(EnvironmentPort::new(env_var))
            }
            AnyEnvironmentVar::Json(env_var) => {
                AnyEnvironmentPort::Json(EnvironmentPort::new(env_var))
            }
        }
    }
}

pub type EnvironmentVar<M> = Arc<RwLock<M>>;

any_message_enum!(AnyEnvironmentVar, EnvironmentVar);

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
/// - [JsonMessage]
pub trait Message: Clone + Send + Sync + 'static {}

/// A message containing an `i32` value.
#[derive(Clone, Debug)]
pub struct IntMessage {
    pub data: i32,
}

impl IntMessage {
    pub fn new(data: i32) -> Self {
        Self { data }
    }
}

impl Message for IntMessage {}

/// A message containing a `f32` value.
#[derive(Clone, Debug)]
pub struct FloatMessage {
    pub data: f32,
}

impl FloatMessage {
    pub fn new(data: f32) -> Self {
        Self { data }
    }
}

impl Message for FloatMessage {}

/// A message containing an `bool` value.
#[derive(Clone, Debug)]
pub struct BoolMessage {
    pub data: bool,
}

impl BoolMessage {
    pub fn new(data: bool) -> Self {
        Self { data }
    }
}

impl Message for BoolMessage {}

/// A message containing an `Vec<u8>` value.
#[derive(Clone, Debug)]
pub struct BytesMessage {
    pub data: Vec<u8>,
}

impl BytesMessage {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl Message for BytesMessage {}

/// A message containing an `String` value.
#[derive(Clone, Debug)]
pub struct TextMessage {
    pub data: String,
}

impl TextMessage {
    pub fn new<S: Into<String>>(data: S) -> Self {
        Self { data: data.into() }
    }
}

impl Message for TextMessage {}

/// A message containing a JSON value.
#[derive(Clone, Debug)]
pub struct JsonMessage {
    pub data: Value,
}

impl JsonMessage {
    pub fn new(data: Value) -> Self {
        Self { data: data }
    }
}

impl Message for JsonMessage {}
