pub mod wasm;
pub mod qemu;
pub mod test;

use std::{any::type_name, collections::HashMap};

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Select, Sender};

pub trait Component<S> {
    fn system_port_hub(&self) -> &dyn SystemPortHub;
    fn system_port_hub_mut(&mut self) -> &mut dyn SystemPortHub;
    fn run(&mut self) -> Result<()>;
}

pub trait SystemPortHub {
    fn port_by_name(&self, name: &str) -> Option<&dyn ConnectablePort>;
    fn port_by_name_mut(&mut self, name: &str) -> Option<&mut dyn ConnectablePort>;
}

pub struct DynamicSystemPortHub<S> {
    ports: HashMap<String, Box<dyn ManagedSystemPort<Self, S> + Send>>,
}

impl<S> DynamicSystemPortHub<S> {
    pub fn new() -> Self {
        DynamicSystemPortHub { ports: HashMap::new() }
    }

    pub fn add_port(&mut self, name: &str, port: Box<dyn ManagedSystemPort<Self, S> + Send>) {
        self.ports.insert(name.into(), port);
    }

    // Would be much cleaner to have a recv_dispatch function to dispatch a single message, but
    // because for some annoying reason Select only takes receivers by references we construct a
    // new one on the fly. And that would cost a lot of performance to do for every receive. It is
    // also impossible to have the Select object a member of the struct to persist it between calls
    // since that would create a self-referenced struct (impossible in safe rust).
    pub fn loop_recv_dispatch(&self, state: &mut S) -> Result<()> {
        let mut sel = Select::new();
        let ports: Vec<_> = self.ports.values().collect();
        for port in &ports {
            port.register(&mut sel);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            ports[index].recv(self, state)?;
        }
    }
}

impl<S> SystemPortHub for DynamicSystemPortHub<S> {
    fn port_by_name(&self, name: &str) -> Option<&dyn ConnectablePort> {
        self.ports.get(name).map(|v| v.as_connectable_port())
    }

    fn port_by_name_mut<'a>(&'a mut self, name: &str) -> Option<&'a mut (dyn ConnectablePort + 'a)> {
        self.ports.get_mut(name).map(|v| v.as_connectable_port_mut() as &mut (dyn ConnectablePort + 'a))
    }
}

#[derive(Clone, Debug)]
pub enum PortQueueCapacity {
    Unbounded,
    Bounded(usize),
}

#[derive(Debug)]
pub enum GenericSender {
    Int(Sender<IntMessage>),
    Float(Sender<FloatMessage>),
    Bool(Sender<BoolMessage>),
    Bytes(Sender<BytesMessage>),
    Text(Sender<TextMessage>),
    Json(Sender<JsonMessage>),
}

#[derive(Debug)]
pub enum GenericMessage {
    Int(IntMessage),
    Float(FloatMessage),
    Bool(BoolMessage),
    Bytes(BytesMessage),
    Text(TextMessage),
    Json(JsonMessage),
}

pub trait Message: Clone + Sync + Send + 'static {
    fn make_generic_message(msg: Self) -> GenericMessage;
    fn make_message(generic_msg: GenericMessage) -> Result<Self>;

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender;
    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>>;
}

#[derive(Clone, Debug)]
pub struct IntMessage {
    pub data: i32,
}

impl IntMessage {
    pub fn new(data: i32) -> Self {
        Self { data }
    }
}

impl Message for IntMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Int(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Int(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Int(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Int(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct FloatMessage {
    pub data: f32,
}

impl FloatMessage {
    pub fn new(data: f32) -> Self {
        Self { data }
    }
}

impl Message for FloatMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Float(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Float(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Float(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Float(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct BoolMessage {
    pub data: bool,
}

impl BoolMessage {
    pub fn new(data: bool) -> Self {
        Self { data }
    }
}

impl Message for BoolMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Bool(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Bool(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Bool(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Bool(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct BytesMessage {
    pub data: Vec<u8>,
}

impl BytesMessage {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl Message for BytesMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Bytes(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Bytes(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Bytes(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Bytes(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextMessage {
    pub data: String,
}

impl TextMessage {
    pub fn new(data: &str) -> Self {
        Self { data: data.into() }
    }
}

impl Message for TextMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Text(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Text(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Text(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Text(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct JsonMessage {
    pub data: String,
}

impl JsonMessage {
    pub fn new(data: &str) -> Self {
        Self { data: data.into() }
    }
}

impl Message for JsonMessage {
    fn make_generic_message(msg: Self) -> GenericMessage {
        GenericMessage::Json(msg)
    }

    fn make_message(generic_msg: GenericMessage) -> Result<Self> {
        match generic_msg {
            GenericMessage::Json(msg) => Ok(msg),
            _ => Err(anyhow!("Message {:?} can't be converted to message type {}.", generic_msg, type_name::<Self>()))
        }
    }

    fn make_generic_sender(tx: Sender<Self>) -> GenericSender {
        GenericSender::Json(tx)
    }

    fn make_sender(endpoint: &GenericSender) -> Result<Sender<Self>> {
        match endpoint {
            GenericSender::Json(tx) => Ok(tx.clone()),
            _ => Err(anyhow!("Endpoint {:?} incompatible with message type {}.", endpoint, type_name::<Self>()))
        }
    }
}


pub trait ConnectablePort {
    fn connect_endpoint(&mut self, endpoint: &GenericSender) -> Result<()>;
    fn get_endpoint(&self) -> GenericSender;
    fn send_generic(&self, msg: GenericMessage) -> Result<()>;
}

pub trait ManagedSystemPort<H: SystemPortHub, S>: ConnectablePort {
    fn register<'a>(&'a self, sel: &mut Select<'a>) -> usize;
    fn recv(&self, hub: &H, state: &mut S) -> Result<()>;

    // [TODO]: remove on Apr 3 with the release of Rust 1.86.0
    fn as_connectable_port(&self) -> &dyn ConnectablePort;
    fn as_connectable_port_mut(&mut self) -> &mut dyn ConnectablePort;
}

pub struct SystemPort<H: SystemPortHub, S, M: Message> {
    rx: Receiver<M>,
    tx: Sender<M>,
    connected_txs: Vec<Sender<M>>,
    recv_callback: Box<dyn Fn(&H, &mut S, &M) -> Result<()> + Send>,}

impl<H: SystemPortHub, S, M: Message> SystemPort<H, S, M> {
    pub fn new(recv_callback: Box<dyn Fn(&H, &mut S, &M) -> Result<()> + Send>, queue_capacity: PortQueueCapacity) -> Self {
        let (tx, rx) = match queue_capacity {
            PortQueueCapacity::Unbounded => unbounded(),
            PortQueueCapacity::Bounded(cap) => bounded(cap),
        };
        Self { rx, tx, connected_txs: Vec::new(), recv_callback }
    }

    fn send(&self, msg: &M) -> Result<()> {
        for tx in &self.connected_txs {
            tx.send(msg.clone())?;
        }
        Ok(())
    }
}

impl<H: SystemPortHub, S, M: Message> ConnectablePort for SystemPort<H, S, M> {
    fn connect_endpoint(&mut self, endpoint: &GenericSender) -> Result<()> {
        self.connected_txs.push(M::make_sender(endpoint)?);
        Ok(())
    }

    fn get_endpoint(&self) -> GenericSender {
        M::make_generic_sender(self.tx.clone())
    }

    fn send_generic(&self, msg: GenericMessage) -> Result<()> {
        let m = M::make_message(msg)?;
        self.connected_txs.iter().skip(1).try_for_each(|tx| tx.send(m.clone()))?;
        if let Some(tx) = self.connected_txs.first() {
            tx.send(m)?;
        }
        Ok(())
    }
}

impl<H: SystemPortHub, S, M: Message> ManagedSystemPort<H, S> for SystemPort<H, S, M> {
    fn register<'a>(&'a self, sel: &mut Select<'a>) -> usize {
        sel.recv(&self.rx)
    }

    fn recv(&self, hub: &H, state: &mut S) -> Result<()> {
        let res = self.rx.recv();
        match res {
            Err(e) => return Err(e.into()),
            Ok(msg) => (self.recv_callback)(hub, state, &msg),
        }
    }

    // [TODO]: remove on Apr 3 with the release of Rust 1.86.0
    fn as_connectable_port(&self) -> &dyn ConnectablePort { self }

    // [TODO]: remove on Apr 3 with the release of Rust 1.86.0
    fn as_connectable_port_mut(&mut self) -> &mut dyn ConnectablePort { self }
}
