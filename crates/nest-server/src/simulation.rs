use std::{collections::{HashMap, HashSet}, time::Instant};

use anyhow::{Result, bail};
use crossbeam_channel::{Receiver, Select, TryRecvError};

use crate::{
    component::{
        Component, ComponentEvent, ComponentHandle, ComponentParams, LogEntry, mux::MuxComponent, qemu::QemuComponent, wasm::WasmComponent
    },
    environment::{AnySharedEnvironmentVar, SharedEnvironmentVar},
    parser::specs::{ComponentTypeSpec, EnvPortTypeSpec, PortTypeSpec, TopologySpec},
    ports::{AnyEnvironmentPort, AnySystemPortBuilder, SystemPortBuilder},
};

pub struct SimulationBuilder {
    components: HashMap<String, Box<dyn Component>>,
    environment: HashMap<String, AnySharedEnvironmentVar>,
}

impl SimulationBuilder {
    pub fn from_topology(topology: &TopologySpec) -> Result<Self> {
        // Component names must be unique, otherwise they silently shadow each other
        let mut component_names: HashSet<&str> = HashSet::new();
        for comp in &topology.components {
            if !component_names.insert(comp.name.as_str()) {
                bail!("Duplicate component name: {}", comp.name);
            }
        }

        // Environment variable names must be unique as well
        let mut env_var_types: HashMap<&str, &EnvPortTypeSpec> = HashMap::new();
        for env_var in &topology.environment {
            if env_var_types
                .insert(env_var.name.as_str(), &env_var.var_type)
                .is_some()
            {
                bail!("Duplicate environment variable name: {}", env_var.name);
            }
        }

        // Create system ports
        let mut system_ports: HashMap<String, HashMap<String, AnySystemPortBuilder>> = HashMap::new();
        for comp in &topology.components {
            let mut comp_ports: HashMap<String, AnySystemPortBuilder> = HashMap::new();
            for port in &comp.system_ports {
                let builder = match port.port_type {
                    PortTypeSpec::Int => AnySystemPortBuilder::Int(SystemPortBuilder::default()),
                    PortTypeSpec::Float => AnySystemPortBuilder::Float(SystemPortBuilder::default()),
                    PortTypeSpec::Bool => AnySystemPortBuilder::Bool(SystemPortBuilder::default()),
                    PortTypeSpec::Bytes => AnySystemPortBuilder::Bytes(SystemPortBuilder::default()),
                    PortTypeSpec::Text => AnySystemPortBuilder::Text(SystemPortBuilder::default()),
                    PortTypeSpec::I2c => AnySystemPortBuilder::I2c(SystemPortBuilder::default()),
                };
                if comp_ports.insert(port.name.clone(), builder).is_some() {
                    bail!(
                        "Duplicate port name {} in component {}",
                        port.name, comp.name
                    );
                }
            }
            system_ports.insert(comp.name.clone(), comp_ports);
        }

        // Connect ports
        let mut connections: HashSet<(String, String)> = HashSet::new();
        for wire in &topology.wires {
            for (i, endpoint1) in wire.endpoints.iter().enumerate() {
                for endpoint2 in &wire.endpoints[i + 1..] {
                    if endpoint1.component == endpoint2.component {
                        bail!("All endpoints of a wire must belong to different components, \
                            but component {} appears twice in the same wire", endpoint1.component);
                    }

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

                    if port1.type_name() != port2.type_name() {
                        bail!("Cannot wire {}.{} (type {}) to {}.{} (type {}): wired ports must have the same type",
                            endpoint1.component, endpoint1.port, port1.type_name(),
                            endpoint2.component, endpoint2.port, port2.type_name());
                    }

                    // Connect both directions, but only once per pair of ports
                    let a = format!("{}.{}", endpoint1.component, endpoint1.port);
                    let b = format!("{}.{}", endpoint2.component, endpoint2.port);
                    let pair = if a <= b { (a, b) } else { (b, a) };
                    if !connections.insert(pair.clone()) {
                        bail!("Duplicate wire between {} and {}", pair.0, pair.1);
                    }

                    port1.connect_to(port2)?;
                    port2.connect_to(port1)?;
                }
            }
        }

        // Create environment
        let mut environment: HashMap<String, AnySharedEnvironmentVar> = HashMap::new();
        for env_var in &topology.environment {
            environment.insert(
                env_var.name.clone(),
                match env_var.var_type {
                    EnvPortTypeSpec::Int => AnySharedEnvironmentVar::Int(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Float => AnySharedEnvironmentVar::Float(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Bool => AnySharedEnvironmentVar::Bool(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Vec2 => AnySharedEnvironmentVar::Vec2(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Vec3 => AnySharedEnvironmentVar::Vec3(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Quaternion => AnySharedEnvironmentVar::Quaternion(SharedEnvironmentVar::default()),
                    EnvPortTypeSpec::Tensor => AnySharedEnvironmentVar::Tensor(SharedEnvironmentVar::default()),
                }
            );
        }

        // Create components
        let mut components: HashMap<String, Box<dyn Component>> = HashMap::new();
        for comp in &topology.components {
            let mut params = ComponentParams::new();

            params.set_config(comp.config.clone());

            if let Some(comp_ports) = system_ports.remove(&comp.name) {
                for (port_name, port_builder) in comp_ports {
                    params.add_system_port(port_name, port_builder.build());
                }
            }

            // Create environment ports
            let mut env_port_names: HashSet<&str> = HashSet::new();
            for env_port in &comp.environment_ports {
                if !env_port_names.insert(env_port.name.as_str()) {
                    bail!(
                        "Duplicate environment port name {} in component {}",
                        env_port.name, comp.name
                    );
                }

                let Some(var_type) = env_var_types.get(env_port.var_name.as_str()) else {
                    bail!("No matching environment variable name for port {}.{}: {}", comp.name, env_port.name, env_port.var_name);
                };

                if *var_type != &env_port.port_type {
                    bail!("Type mismatch for environment port {}.{}: variable {} is of type {}, but the port is declared as type {}",
                        comp.name, env_port.name, env_port.var_name, var_type.name(), env_port.port_type.name());
                }

                let Some(env_var) = environment.get(&env_port.var_name) else {
                    bail!("No matching environment variable name for port {}.{}: {}", comp.name, env_port.name, env_port.var_name);
                };
                params.add_env_port(env_port.name.clone(), AnyEnvironmentPort::from(env_var.clone()));
            }

            match comp.component_type {
                ComponentTypeSpec::Qemu => { components.insert(comp.name.clone(), Box::new(QemuComponent::from_params(params)?)); },
                ComponentTypeSpec::Wasm => { components.insert(comp.name.clone(), Box::new(WasmComponent::from_params(params)?)); },
                ComponentTypeSpec::Mux  => { components.insert(comp.name.clone(), Box::new(MuxComponent::from_params(params)?)); },
            }
        }

        Ok(SimulationBuilder { components, environment })
    }

    pub fn start_simulation(self) -> Result<Simulation> {
        let sim_start_time = Instant::now();
        let handles = self.components
            .into_iter()
            .map(|(name, component)| Ok((name, component.start(sim_start_time)?)))
            .collect::<Result<_>>()?;

        Ok(Simulation::new(handles, self.environment, sim_start_time))
    }
}

pub struct Simulation {
    component_handles: HashMap<String, ComponentHandle>,
    environment: HashMap<String, AnySharedEnvironmentVar>,
    start_time: Instant,
}

impl Simulation {
    pub fn new(
        component_handles: HashMap<String, ComponentHandle>,
        environment: HashMap<String, AnySharedEnvironmentVar>,
        start_time: Instant
    ) -> Self {
        Self {
            component_handles,
            environment,
            start_time,
        }
    }

    pub fn handle(&self, name: &str) -> Option<&ComponentHandle> {
        self.component_handles.get(name)
    }

    pub fn env_var(&self, name: &str) -> Option<&AnySharedEnvironmentVar> {
        self.environment.get(name)
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

    /// Return the instant the simulation was started.
    pub fn started_at(&self) -> Instant {
        self.start_time
    }
}

pub struct SimulationReceiver<T> {
    receivers: Vec<(String, Receiver<T>)>,
}

impl<T> SimulationReceiver<T> {
    pub fn new(receivers: Vec<(String, Receiver<T>)>) -> Self {
        Self { receivers }
    }

    pub fn recv(&mut self) -> Option<(String, T)> {
        let mut sel = Select::new();

        for (_source, receiver) in &self.receivers {
            sel.recv(receiver);
        }

        let mut disconnected = Vec::new();
        let msg = loop {
            if disconnected.len() >= self.receivers.len() { break None }
            let index = sel.ready();
            let (source, receiver) = &self.receivers[index];

            match receiver.try_recv() {
                Ok(msg) => break Some((source.into(), msg)),
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => {
                    disconnected.push(index);
                    sel.remove(index);
                }
            }
        };

        disconnected.sort_unstable_by(|a, b| b.cmp(a));
        for i in disconnected {
            self.receivers.swap_remove(i);
        }

        msg
    }
}

