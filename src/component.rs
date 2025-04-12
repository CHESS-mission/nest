pub mod wasm;
pub mod qemu;
pub mod test;

use std::{sync::{Arc, Mutex}, thread::JoinHandle};

use anyhow::Result;
use crossbeam_channel::{bounded, unbounded, Receiver, Select, Sender};

pub trait HasPortType<M: Message> {
    fn port_by_name(&self, _name: &str) -> Option<&dyn ConnectablePort<M>> {
        None
    }

    fn port_by_name_mut(&mut self, _name: &str) -> Option<&mut dyn ConnectablePort<M>> {
        None
    }
}

pub trait Component:
    Send
    + HasPortType<IntMessage>
    + HasPortType<FloatMessage>
    + HasPortType<BoolMessage>
    + HasPortType<BytesMessage>
    + HasPortType<TextMessage>
    + HasPortType<JsonMessage>
{
    fn start(self) -> Result<ComponentMonitor>;
}

pub enum MonitorCommand {
    Stop,
    StartObservableStateUpdates,
    StopObservableStateUpdates,
}

pub struct ComponentMonitor {
    handle: JoinHandle<Result<()>>,
    tx: Sender<MonitorCommand>,
    observable_state: Arc<Mutex<String>>,
}

impl ComponentMonitor {
    pub fn new(handle: JoinHandle<Result<()>>, tx: Sender<MonitorCommand>, observable_state: Arc<Mutex<String>>) -> Self {
        Self { handle, tx, observable_state }
    }

    pub fn stop(self) -> Result<()> {
        self.tx.send(MonitorCommand::Stop)?;
        self.handle.join().unwrap().unwrap();
        Ok(())
    }

    pub fn start_observable_state_updates(&self) -> Result<()> {
        self.tx.send(MonitorCommand::StartObservableStateUpdates)?;
        Ok(())
    }

    pub fn stop_observable_state_updates(&self) -> Result<()> {
        self.tx.send(MonitorCommand::StopObservableStateUpdates)?;
        Ok(())
    }

    pub fn observable_state(&self) -> String {
        self.observable_state.lock().unwrap().clone()
    }
}

#[derive(Clone, Debug)]
pub enum PortQueueCapacity {
    Unbounded,
    Bounded(usize),
}

pub trait ConnectablePort<M: Message> {
    fn connect_to(&mut self, other: &dyn ConnectablePort<M>);
    fn sender(&self) -> &Sender<M>;
    fn send(&self, msg: &M) -> Result<()>;
}

pub trait ManagedSystemPort<H, S> {
    fn register<'a>(&'a self, sel: &mut Select<'a>) -> usize;
    fn recv(&self, hub: &H, state: &mut S) -> Result<()>;
}

pub struct SystemPort<H, S, M: Message> {
    rx: Receiver<M>,
    tx: Sender<M>,
    connected_txs: Vec<Sender<M>>,
    recv_callback: Box<dyn Fn(&H, &mut S, &M) -> Result<()> + Send>,
}

impl<H, S, M: Message> SystemPort<H, S, M> {
    pub fn new(recv_callback: Box<dyn Fn(&H, &mut S, &M) -> Result<()> + Send>, queue_capacity: PortQueueCapacity) -> Self {
        let (tx, rx) = match queue_capacity {
            PortQueueCapacity::Unbounded => unbounded(),
            PortQueueCapacity::Bounded(cap) => bounded(cap),
        };
        Self { rx, tx, connected_txs: Vec::new(), recv_callback }
    }
}

impl<H, S, M: Message> ConnectablePort<M> for SystemPort<H, S, M> {
    fn connect_to(&mut self, other: &dyn ConnectablePort<M>) {
        self.connected_txs.push(other.sender().clone());
    }

    fn sender(&self) -> &Sender<M> {
        &self.tx
    }

    fn send(&self, msg: &M) -> Result<()> {
        for tx in &self.connected_txs {
            tx.send(msg.clone())?;
        }
        Ok(())
    }
}

impl<H, S, M: Message> ManagedSystemPort<H, S> for SystemPort<H, S, M> {
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
}

// Messages

pub trait Message: Clone + Sync + Send + 'static { }

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

#[derive(Clone, Debug)]
pub struct JsonMessage {
    pub data: String,
}

impl JsonMessage {
    pub fn new(data: &str) -> Self {
        Self { data: data.into() }
    }
}

impl Message for JsonMessage { }
