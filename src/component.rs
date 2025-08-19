pub mod wasm;
pub mod qemu;
pub mod test;

use std::{collections::HashMap, sync::{Arc, Mutex}, thread::{self, JoinHandle}, time::Instant};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Select, Sender};
use serde_json::Value;

use crate::ports::{GenericSystemPort, Message, SystemPort};

// Component

/// Trait implemented by components.
///
/// The start method is called by the simulator to start executing the component and returns a
/// [ComponentMonitor].
///
/// The from_params method must allow creating new instances of the component from a
/// [ComponentParams].
pub trait Component: Send {
    /// Start executing the component on a new thread.
    ///
    /// Return a [ComponentMonitor] with a handle to the spawned thread.
    fn start(self) -> Result<ComponentMonitor>;

    /// Creates a new instance of the component from a [ComponentParams].
    /// The method should return an error if invalid parameter are passed.
    fn from_params(params: &ComponentParams) -> Result<Self>
    where
        Self: Sized;
}

/// Component parameters (key-value based).
#[derive(Debug)]
pub struct ComponentParams {
    config: serde_json::Value,
    system_ports: HashMap<String, GenericSystemPort>,
}

impl ComponentParams {
    /// Create a new empty [ComponentParams].
    pub fn new() -> Self {
        Self { config: serde_json::Value::Null, system_ports: HashMap::new() }
    }
    
    /// Get a reference to the config from the parameters.
    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }
    
    /// Get a mutable reference to the config from the parameters.
    pub fn config_mut(&mut self) -> &mut serde_json::Value {
        &mut self.config
    }
    
    /// Overwrite config.
    pub fn set_config(&mut self, config: serde_json::Value) {
        self.config = config
    }
    
    /// Get a [GenericSystemPort] from the parameters if it exists.
    pub fn add_system_port<K: Into<String>>(&mut self, key: K, port: GenericSystemPort) {
        self.system_ports.insert(key.into(), port);
    }
    
    /// Get a [GenericSystemPort] from the parameters if it exists.
    pub fn system_port(&self, key: &str) -> Option<&GenericSystemPort> {
        self.system_ports.get(key)
    }
    
    /// Get a mutable reference to a [GenericSystemPort] from the parameters if it exists.
    pub fn system_port_mut(&mut self, key: &str) -> Option<&mut GenericSystemPort> {
        self.system_ports.get_mut(key)
    }
    
    /// Get a vector of all system port key-value pairs from the parameters.
    pub fn system_ports(&self) -> Vec<(&String, &GenericSystemPort)> {
        self.system_ports.iter().collect()
    }
}

// MessageDispatcher

/// A utility struct that registers a set of receivers and callbacks, and implements the
/// dispatching logic for components to deliver messages from the receivers to the associated associated
/// callbacks.
///
/// The callbacks are passed a custom mutable object in addition to the received message. They
/// return a Result with [HandlerStatus::Stop] if the dispatcher should stop and [HandlerStatus::Ok] otherwise.
pub struct MessageDispatcher<'a, S> {
    message_handlers: Vec<Box<dyn MessageHandler<S> + 'a>>,
}

impl<'a, S: 'a> MessageDispatcher<'a, S> {
    /// Create a new [MessageDispatcher].
    pub fn new() -> Self {
        Self { message_handlers: Vec::new() }
    }

    /// Register a new receiver and callback.
    pub fn register<T: Send + 'static>(&mut self, receiver: Receiver<T>, callback: Box<dyn Fn(T, &mut S) -> Result<HandlerStatus> + Send + 'a>) {
        let handler = TypedMessageHandler::new(receiver, callback);
        self.message_handlers.push(Box::new(handler));
    }

    /// Register a new [SystemPort] and callback.
    pub fn register_system_port<M: Message>(&mut self, system_port: &SystemPort<M>, callback: Box<dyn Fn(M, &mut S) -> Result<HandlerStatus> + Send + 'a>) {
        self.register(system_port.receiver().clone(), callback);
    }

    /// Start dispatching messages in a loop until a callback returns an error or
    /// [HandlerStatus::Stop].
    pub fn start_dispatch(&mut self, mut state: S) -> Result<()> {
        let mut sel = Select::new();

        // Register all receivers
        for handler in &self.message_handlers {
            handler.register(&mut sel);
        }

        loop {
            // Wait until a receive operation becomes ready and handle it
            let index = sel.ready();
            let status = self.message_handlers[index].handle(&mut state)?;

            // Shut down dispatcher if status is Stop
            if let HandlerStatus::Stop = status { return Ok(()) }
        }
    }
}

/// The return status of a callback function from [MessageDispatcher].
pub enum HandlerStatus {
    /// The callback was successfully executed.
    Ok,
    /// Signals the [MessageDispatcher] to stop.
    Stop,
}

trait MessageHandler<S>: Send {
    fn register<'s, 'h>(&'h self, sel: &mut Select<'s>) where 'h: 's;
    fn handle(&self, state: &mut S) -> Result<HandlerStatus>;
}

struct TypedMessageHandler<'a, T, S> {
    receiver: Receiver<T>,
    callback: Box<dyn Fn(T, &mut S) -> Result<HandlerStatus> + Send + 'a>,
}

impl<'a, T, S> TypedMessageHandler<'a, T, S> {
    fn new(receiver: Receiver<T>, handler: Box<dyn Fn(T, &mut S) -> Result<HandlerStatus> + Send + 'a>) -> Self {
        Self { receiver, callback: handler }
    }
}

impl<'a, T: Send + 'static, S> MessageHandler<S> for TypedMessageHandler<'a, T, S> {
    fn register<'s, 'h>(&'h self, sel: &mut Select<'s>)
    where 
        'h: 's,
    {
        sel.recv(&self.receiver);
    }

    fn handle(&self, state: &mut S) -> Result<HandlerStatus> {
        match self.receiver.recv() {
            Ok(msg) => (self.callback)(msg, state),
            Err(e) => Err(e.into()),
        }
    }
}

// Logging

/// The severity level of log entries.
#[derive(Clone, Debug)]
pub enum LogLevel {
    /// Simple information.
    Info,
    /// Indicate a potential issue or unintended behavior.
    Warning,
    /// Indicate an issue.
    Error,
}

/// A log entry with severity (see [LogLevel]), timestamp and message.
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// The log severity level.
    pub level: LogLevel,
    /// The log timestamp.
    pub timestamp: Instant,
    /// The log message.
    pub message: String,
}

impl LogEntry {
    /// Create a new log entry.
    pub fn new(level: LogLevel, timestamp: Instant, message: String) -> Self {
        Self { level, timestamp, message }
    }
}

/// Convenience object for components to send logs to their [ComponentMonitor].
#[derive(Clone, Debug)]
pub struct Logger {
    tx: Sender<LogEntry>,
}

impl Logger {
    /// Create a new [Logger].
    pub fn new(tx: Sender<LogEntry>) -> Self {
        Self { tx }
    }

    /// Log information message.
    pub fn info(&self, message: &str) -> Result<()> {
        self.tx.send(LogEntry::new(LogLevel::Info, Instant::now(), message.to_string()))?;
        Ok(())
    }

    /// Log warning message.
    pub fn warning(&self, message: &str) -> Result<()> {
        self.tx.send(LogEntry::new(LogLevel::Warning, Instant::now(), message.to_string()))?;
        Ok(())
    }

    /// Log error message.
    pub fn error(&self, message: &str) -> Result<()> {
        self.tx.send(LogEntry::new(LogLevel::Error, Instant::now(), message.to_string()))?;
        Ok(())
    }
}

// ComponentMonitor

/// A handle to monitor the execution of a [Component]. It allows to send commands (see [MonitorCommand]),
/// receive logs (see [LogEntry]) and read the component's status.
#[derive(Debug)]
pub struct ComponentMonitor {
    join_handle: JoinHandle<Result<()>>,
    command_sender: Sender<MonitorCommand>,
    event_receiver: Receiver<ComponentEvent>,
    log_receiver: Receiver<LogEntry>,
    shared_status: Arc<Mutex<Value>>,
}

impl ComponentMonitor {
    /// Create a new [ComponentMonitor] from the handle of the component's thread, a command
    /// sender, a log receiver and the shared status object of the component.
    pub fn new(
        join_handle: JoinHandle<Result<()>>,
        command_sender: Sender<MonitorCommand>,
        event_receiver: Receiver<ComponentEvent>,
        log_receiver: Receiver<LogEntry>,
        shared_status: Arc<Mutex<Value>>) -> Self
    {
        Self { join_handle, command_sender, event_receiver, log_receiver, shared_status }
    }

    /// Spawn the function `f` on a separate thread and returns a new [ComponentMonitor] to monitor
    /// it. The function `f` receives the command receiver, [Logger], and shared status reference as
    /// arguments.
    pub fn start_monitor<F>(f: F) -> Self
    where
        F: FnOnce(Receiver<MonitorCommand>, Sender<ComponentEvent>, Logger, Arc<Mutex<Value>>) -> Result<()>,
        F: Send + 'static,
    {
        let (command_tx, command_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        let (log_tx, log_rx) = unbounded();
        let status = Arc::new(Mutex::new(Value::Null));
        let status_clone = status.clone();
        let handle = thread::spawn(move || {
            if let Err(e) = f(command_rx, event_tx.clone(), Logger::new(log_tx), status_clone) {
                event_tx.send(ComponentEvent::Error(e.to_string()))?;
            }
            Ok(())
        });
        ComponentMonitor::new(handle, command_tx, event_rx, log_rx, status)
    }

    /// Request the component to stop. Note that the command will be queued and the component might
    /// not instantly stop.
    pub fn stop(self) -> Result<()> {
        self.command_sender.send(MonitorCommand::Stop)?;
        self.join_handle.join().unwrap().unwrap();
        Ok(())
    }

    /// Request the component to start updating its status. Components are expected to not update
    /// their status by default for performance reasons. Note that the command will be queued and
    /// might not immediately take effect.
    pub fn start_status_updates(&self) -> Result<()> {
        self.command_sender.send(MonitorCommand::StartStatusUpdates)?;
        Ok(())
    }

    /// Request the component to stop updating its status. Components are expected to not update
    /// their status by default for performance reasons. Note that the command will be queued and
    /// might not immediately take effect.
    pub fn stop_status_updates(&self) -> Result<()> {
        self.command_sender.send(MonitorCommand::StopStatusUpdates)?;
        Ok(())
    }

    /// Issue a simulator command to the component.
    pub fn simulator_command<S: Into<String>>(&self, command: S, args: serde_json::Value) -> Result<()> {
        self.command_sender.send(MonitorCommand::SimulatorCommand { command: command.into(), args })?;
        Ok(())
    }

    /// Read the component status and return a copy. Requires cloning but keeps the lock for the
    /// shortest amount of time.
    pub fn status(&self) -> Result<Value> {
        self.shared_status.lock()
            .map(|guard| guard.clone())
            .map_err(|_| anyhow!("Failed to acquire lock"))
    }

    /// Return a shared reference to the component status. Only useful for reading a small part. 
    /// Note that the status should not be mutated outside of the component.
    pub fn status_shared(&self) -> Arc<Mutex<Value>> {
        Arc::clone(&self.shared_status)
    }

    /// Return a reference to the monitor's log receiver. Can be cloned safely.
    pub fn log_receiver(&self) -> &Receiver<LogEntry> {
        &self.log_receiver
    }
}

/// Command sent from a [ComponentMonitor] to a [Component].
#[derive(Clone, Debug)]
pub enum MonitorCommand {
    /// Requests the component to stop.
    Stop,
    /// Requests the component to start updating its status.
    StartStatusUpdates,
    /// Requests the component to stop updating its status.
    StopStatusUpdates,
    /// Issues a simulator command to the component.
    SimulatorCommand { command: String, args: serde_json::Value },
}

impl MonitorCommand {
    /// Create a new [MonitorCommand::SimulatorCommand].
    pub fn sim_command<C, A>(command: C, args: A) -> Self
    where
        C: Into<String>,
        A: Into<Value>,
    {
        Self::SimulatorCommand {
            command: command.into(),
            args: args.into(),
        }
    }
}

/// Event sent from a [Component] to a [ComponentMonitor].
#[derive(Clone, Debug)]
pub enum ComponentEvent {
    /// Signals an unrecoverable error to the monitor. The component is expected to immediately
    /// stop after sending a [ComponentEvent::Error].
    Error(String),
    /// Reports a simulation event to the simulator.
    SimulationEvent { event: String, value: serde_json::Value },
}

impl ComponentEvent {
    /// Create a new [ComponentEvent::Error].
    pub fn error<E: Into<String>>(error: E) -> Self {
        Self::Error(error.into())
    }

    /// Create a new [ComponentEvent::SimulationEvent].
    pub fn sim_event<E, V>(event: E, value: V) -> Self
    where
        E: Into<String>,
        V: Into<Value>,
    {
        Self::SimulationEvent {
            event: event.into(),
            value: value.into(),
        }
    }
}
