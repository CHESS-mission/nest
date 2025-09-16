use std::{collections::HashMap, time::SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum NestApiCommand {
    Start,
    Stop,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum NestApiEvent {
    SimulatorState(SimulatorState),
    Log(Log),
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Log {
    pub source: String,
    pub level: LogLevel,
    pub timestamp: SystemTime,
    pub message: String,
}

/// The severity level of log entries.
#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum LogLevel {
    /// Simple information.
    Info,
    /// Indicate a potential issue or unintended behavior.
    Warning,
    /// Indicate an issue.
    Error,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum SimulatorState {
    Idle,
    Ready {
        topology: Topology,
    },
    Running {
        topology: Topology,
        started_time: SystemTime,
    },
}

impl SimulatorState {
    pub fn is_idle(&self) -> bool {
        matches!(self, SimulatorState::Idle)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, SimulatorState::Ready {..})
    }
    
    pub fn is_running(&self) -> bool {
        matches!(self, SimulatorState::Running {..})
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Topology {
    pub components: HashMap<String, Component>,
    pub wires: Vec<Wire>,
}

impl Topology {
    pub fn new(components: HashMap<String, Component>, wires: Vec<Wire>) -> Self {
        Self { components, wires }
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Component {
    pub kind: ComponentKind,
    pub config: serde_json::Value,
    pub system_ports: Vec<(String, PortType)>,
    pub env_ports: Vec<(String, PortType)>,
}

impl Component {
    pub fn new(kind: ComponentKind, config: serde_json::Value, system_ports: Vec<(String, PortType)>, env_ports: Vec<(String, PortType)>) -> Self {
        Self { kind, config, system_ports, env_ports }
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum ComponentKind {
    Qemu,
    Wasm,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum PortType {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    Json,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Wire {
    pub endpoints: Vec<WireEndpoint>,
}

impl Wire {
    pub fn new(endpoints: Vec<WireEndpoint>) -> Self {
        Self { endpoints }
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct WireEndpoint {
    pub component: String,
    pub port: String,
}

impl WireEndpoint {
    pub fn new<C: Into<String>, P: Into<String>>(component: C, port: P) -> Self {
        Self { component: component.into(), port:  port.into() }
    }
}
