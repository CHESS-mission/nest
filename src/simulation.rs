use std::{collections::HashMap};

use anyhow::{Result, bail};
use crossbeam_channel::{Receiver, Select, TryRecvError};

use crate::{
    component::{
        qemu::QemuComponent, wasm::WasmComponent, Component, ComponentEvent, ComponentHandle, ComponentParams, LogEntry, LogLevel
    },
    ports::{AnySystemPortBuilder, SystemPortBuilder},
};

pub struct SimulationBuilder {
    components: HashMap<String, Box<dyn Component>>,
}

impl SimulationBuilder {
    pub fn from_topology(topology: &Topology) -> Result<Self> {
        // Create system ports
        let mut system_ports: HashMap<String, HashMap<String, AnySystemPortBuilder>> = HashMap::new();
        for (name, raw_comp) in &topology.components {
            system_ports.insert(name.to_owned(), raw_comp.system_ports
                .iter()
                .map(|(port_name, port_type)| (port_name.to_owned(), match port_type {
                    PortType::Int => AnySystemPortBuilder::Int(SystemPortBuilder::default()),
                    PortType::Float => AnySystemPortBuilder::Float(SystemPortBuilder::default()),
                    PortType::Bool => AnySystemPortBuilder::Bool(SystemPortBuilder::default()),
                    PortType::Bytes => AnySystemPortBuilder::Bytes(SystemPortBuilder::default()),
                    PortType::Text => AnySystemPortBuilder::Text(SystemPortBuilder::default()),
                    PortType::Json => AnySystemPortBuilder::Json(SystemPortBuilder::default()),
                }))
                .collect()
            );
        }

        // Connect ports
        for wire in &topology.wires {
            for (i, endpoint1) in wire.endpoints.iter().enumerate() {
                for endpoint2 in &wire.endpoints[i + 1..] {
                    let [comp1_opt, comp2_opt] = system_ports.get_disjoint_mut([
                        &endpoint1.component,
                        &endpoint2.component,
                    ]);
                    
                    let Some(comp1) = comp1_opt else {
                        bail!("Component {} referenced in wire does not exist in topology.", endpoint1.component);
                    };
                    let Some(comp2) = comp2_opt else {
                        bail!("Component {} referenced in wire does not exist in topology.", endpoint2.component);
                    };

                    let Some(port1) = comp1.get_mut(&endpoint1.port) else {
                        bail!("Use of undeclared port {} of component {} in wire.", endpoint1.port, endpoint1.component);
                    };
                    let Some(port2) = comp2.get_mut(&endpoint2.port) else {
                        bail!("Use of undeclared port {} of component {} in wire.", endpoint2.port, endpoint2.component);
                    };

                    port1.connect_to(port2)?;
                    port2.connect_to(port1)?;
                }
            }
        }

        // TODO: create environment

        // Create components
        let mut components: HashMap<String, Box<dyn Component>> = HashMap::new();
        for (name, raw_comp) in &topology.components {
            let mut params = ComponentParams::new();

            params.set_config(raw_comp.config.clone());

            if let Some(comp_ports) = system_ports.remove(name) {
                for (port_name, port_builder) in comp_ports {
                    params.add_system_port(port_name, port_builder.build());
                }
            }

            // TODO: create environment ports

            match raw_comp.kind {
                ComponentKind::Qemu => { components.insert(name.into(), Box::new(QemuComponent::from_params(&params)?)); },
                ComponentKind::Wasm => { components.insert(name.into(), Box::new(WasmComponent::from_params(&params)?)); },
            }
        }


        Ok(SimulationBuilder { components })
    }

    pub fn start_simulation(self) -> Result<Simulation> {
        let handles = self.components
            .into_iter()
            .map(|(name, component)| Ok((name, component.start()?)))
            .collect::<Result<_>>()?;

        Ok(Simulation::new(handles))
    }
}

pub struct Simulation {
    component_handles: HashMap<String, ComponentHandle>,
}

impl Simulation {
    pub fn new(component_handles: HashMap<String, ComponentHandle>) -> Self {
        Self { component_handles }
    }

    pub fn handle(&self, name: &str) -> Option<&ComponentHandle> {
        self.component_handles.get(name)
    }

    pub fn stop(self) -> Result<()> {
        for (_name, handle) in self.component_handles {
            handle.stop()?;
        }

        Ok(())
    }

    /// Return a [SimulationReceiver] for transparently receiving logs from all components.
    /// 
    /// Note that multiple simulation log receivers will compete instead of each receiving all logs.
    pub fn log_receiver(&self) -> SimulationReceiver<LogEntry> {
        SimulationReceiver::new(
            self.component_handles
                .iter()
                .map(|(name, handle)| (name.clone(), handle.log_receiver().clone()))
                .collect()
        )
    }

    /// Return a [SimulationReceiver] for transparently receiving events from all components.
    /// 
    /// Note that multiple simulation event receivers will compete instead of each receiving all logs.
    pub fn event_receiver(&self) -> SimulationReceiver<ComponentEvent> {
        SimulationReceiver::new(
            self.component_handles
                .iter()
                .map(|(name, handle)| (name.clone(), handle.event_receiver().clone()))
                .collect()
        )
    }
}

pub struct SimulationReceiver<T> {
    receivers: Vec<(String, Receiver<T>)>,
}

impl<T> SimulationReceiver<T> {
    pub fn new(receivers: Vec<(String, Receiver<T>)>) -> Self {
        Self { receivers }
    }

    pub fn recv(&self) -> (String, T) {
        let mut sel = Select::new();

        for (_source, receiver) in &self.receivers {
            sel.recv(receiver);
        }

        loop {
            let index = sel.ready();
            let (source, receiver) = &self.receivers[index];

            match receiver.try_recv() {
                Ok(msg) => return (source.into(), msg),
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => sel.remove(index),
            }
        }
    }

    pub fn recv_drain(&mut self) -> Vec<(String, T)> {
        let mut sel = Select::new();

        for (_source, receiver) in &self.receivers {
            sel.recv(receiver);
        }

        let mut msgs = Vec::new();

        // Helper to handle a single receiver result
        let mut handle_recv = |index: usize, sel: &mut Select| {
            let (source, receiver) = &self.receivers[index];
            match receiver.try_recv() {
                Ok(msg) => msgs.push((source.into(), msg)),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => sel.remove(index),
            }
            true
        };

        // Wait for at least one entry
        while !handle_recv(sel.ready(), &mut sel) {};

        // Drain the rest without blocking
        while let Ok(index) = sel.try_ready() {
            handle_recv(index, &mut sel);
        }

        msgs
    }
}

// Work in progress
// TODO: Coordinate with parser

pub struct Topology {
    pub components: HashMap<String, RawComponent>,
    pub wires: Vec<Wire>,
}

impl Topology {
    pub fn new(components: HashMap<String, RawComponent>, wires: Vec<Wire>) -> Self {
        Self { components, wires }
    }
}

pub struct RawComponent {
    pub kind: ComponentKind,
    pub config: serde_json::Value,
    pub system_ports: Vec<(String, PortType)>,
    pub env_ports: Vec<(String, PortType)>,
}

impl RawComponent {
    pub fn new(kind: ComponentKind, config: serde_json::Value, system_ports: Vec<(String, PortType)>, env_ports: Vec<(String, PortType)>) -> Self {
        Self { kind, config, system_ports, env_ports }
    }
}

pub enum ComponentKind {
    Qemu,
    Wasm,
}

pub enum PortType {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    Json,
}

pub struct Wire {
    pub endpoints: Vec<WireEndpoint>,
}

impl Wire {
    pub fn new(endpoints: Vec<WireEndpoint>) -> Self {
        Self { endpoints }
    }
}

pub struct WireEndpoint {
    pub component: String,
    pub port: String,
}

impl WireEndpoint {
    pub fn new<C: Into<String>, P: Into<String>>(component: C, port: P) -> Self {
        Self { component: component.into(), port:  port.into() }
    }
}
