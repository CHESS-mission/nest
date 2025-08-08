// SystemPort

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use serde_json::Value;

/// Simple wrapper around crossbeam_channel receiver (one) and senders (many) with a Message type.
#[derive(Clone, Debug)]
pub struct SystemPort<M: Message> {
    rx: Receiver<M>,
    local_tx: Sender<M>,
    connections: Vec<Sender<M>>,
}

impl<M: Message> SystemPort<M> {
    /// Create a new [SystemPort] of given queue capacity.
    pub fn new(queue_capacity: PortQueueCapacity) -> Self {
        let (local_tx, rx) = match queue_capacity {
            PortQueueCapacity::Unbounded => unbounded(),
            PortQueueCapacity::Bounded(cap) => bounded(cap),
        };
        Self { rx, local_tx, connections: Vec::new() }
    }

    /// Connect the port to another [SystemPort].
    ///
    /// Note: this only means that messages sent through this port will be received
    /// by the other port, but not the other way around. For bi-directional connections,
    /// the other port should also connect to this port.
    pub fn connect_to(&mut self, other: &SystemPort<M>) {
        self.connections.push(other.local_tx.clone());
    }

    /// Send a message through the port.
    pub fn send(&self, msg: &M) -> Result<()> {
        for tx in &self.connections {
            tx.send(msg.clone())?;
        }
        Ok(())
    }

    /// Return a reference to the port's receiver
    pub fn receiver(&self) -> &Receiver<M> {
        &self.rx
    }
}

impl<M: Message> Default for SystemPort<M> {
    fn default() -> Self {
        Self::new(PortQueueCapacity::Unbounded)
    }
}

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

/// A generic wrapper around [SystemPort] to erase the underlying type.
/// Generic system ports are useful when working with heterogeneous [SystemPort] collections, but
/// are not efficient since they require looking up the underlying type for most operations.
#[derive(Clone, Debug)]
pub enum GenericSystemPort {
    Int(SystemPort<IntMessage>),
    Float(SystemPort<FloatMessage>),
    Bool(SystemPort<BoolMessage>),
    Bytes(SystemPort<BytesMessage>),
    Text(SystemPort<TextMessage>),
    Json(SystemPort<JsonMessage>),
}

macro_rules! impl_as_methods {
    ($variant:ident, $method:ident, $method_mut:ident, $msg:ty) => {
        #[doc = concat!("Return a reference to the underlying [`SystemPort<", stringify!($msg), ">`].",
                        " Fails if not a [`GenericSystemPort<", stringify!($variant), ">`].")]
        pub fn $method(&self) -> Result<&SystemPort<$msg>> {
            match self {
                GenericSystemPort::$variant(port) => Ok(port),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!(GenericSystemPort::$variant),
                    stringify!(SystemPort<$msg>)
                )),
            }
        }

        #[doc = concat!("Return a mutable reference to the underlying [`SystemPort<", stringify!($msg), ">`].",
                        " Fails if not a [`GenericSystemPort<", stringify!($variant), ">`].")]
        pub fn $method_mut(&mut self) -> Result<&mut SystemPort<$msg>> {
            match self {
                GenericSystemPort::$variant(port) => Ok(port),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!(GenericSystemPort::$variant),
                    stringify!(SystemPort<$msg>)
                )),
            }
        }
    };
}

macro_rules! impl_into_method {
    ($variant:ident, $method:ident, $msg:ty) => {
        #[doc = concat!("Convert the port into the underlying [`SystemPort<", stringify!($msg), ">`].",
                        " Fails if not a [`GenericSystemPort<", stringify!($variant), ">`].")]
        pub fn $method(self) -> Result<SystemPort<$msg>> {
            match self {
                GenericSystemPort::$variant(port) => Ok(port),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!(GenericSystemPort::$variant),
                    stringify!(SystemPort<$msg>)
                )),
            }
        }
    };
}

impl GenericSystemPort {
    impl_as_methods!(Int, as_int, as_int_mut, IntMessage);
    impl_as_methods!(Float, as_float, as_float_mut, FloatMessage);
    impl_as_methods!(Bool, as_bool, as_bool_mut, BoolMessage);
    impl_as_methods!(Bytes, as_bytes, as_bytes_mut, BytesMessage);
    impl_as_methods!(Text, as_text, as_text_mut, TextMessage);
    impl_as_methods!(Json, as_json, as_json_mut, JsonMessage);

    impl_into_method!(Int, into_int, IntMessage);
    impl_into_method!(Float, into_float, FloatMessage);
    impl_into_method!(Bool, into_bool, BoolMessage);
    impl_into_method!(Bytes, into_bytes, BytesMessage);
    impl_into_method!(Text, into_text, TextMessage);
    impl_into_method!(Json, into_json, JsonMessage);

    /// Connect the port to another [GenericSystemPort]. Fails if the underlying ports are of
    /// different types.
    ///
    /// Note: this only means that messages sent through this port will be received
    /// by the other port, but not the other way around. For bi-directional connections,
    /// the other port should also connect to this port.
    pub fn connect_to(&mut self, other: &GenericSystemPort) -> Result<()> {
        match self {
            GenericSystemPort::Int(port) => port.connect_to(other.as_int()?),
            GenericSystemPort::Float(port) => port.connect_to(other.as_float()?),
            GenericSystemPort::Bool(port) => port.connect_to(other.as_bool()?),
            GenericSystemPort::Bytes(port) => port.connect_to(other.as_bytes()?),
            GenericSystemPort::Text(port) => port.connect_to(other.as_text()?),
            GenericSystemPort::Json(port) => port.connect_to(other.as_json()?),
        }
        Ok(())
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
/// - [JsonMessage]
pub trait Message: Clone + Send + Sync + 'static { }

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

impl Message for IntMessage { }

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

impl Message for FloatMessage { }

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

impl Message for BoolMessage { }

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

impl Message for BytesMessage { }

/// A message containing an `String` value.
#[derive(Clone, Debug)]
pub struct TextMessage {
    pub data: String,
}

impl TextMessage {
    pub fn new(data: &str) -> Self {
        Self { data: data.into() }
    }
}

impl Message for TextMessage { }

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

impl Message for JsonMessage { }
