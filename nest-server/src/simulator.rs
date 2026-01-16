use std::{mem, time::Instant};

use thiserror::Error;

use crate::{component::{ComponentEvent, ComponentHandle, LogEntry}, parser::specs::TopologySpec, simulation::{Simulation, SimulationBuilder, SimulationReceiver}};

/// Custom errors for simulator operations.
#[derive(Debug, Error)]
pub enum SimulatorError {
    #[error("Invalid state for operation: simulation is already running")]
    AlreadyRunning,
    #[error("Invalid state for operation: simulation is not running")]
    NotRunning,
    #[error("Invalid state for operation: no topology loaded")]
    NotReady,
    #[error("Invalid state for operation: a topology is already loaded")]
    AlreadyReady,
    #[error("No topology loaded")]
    NoTopologyLoaded,
    #[error("Failed to initialize simulation from topology: {0}")]
    LoadTopologyFailed(anyhow::Error),
    #[error("Failed to start the simulation: {0}")]
    StartSimulationFailed(anyhow::Error),
    #[error("Failed to stop the simulation: {0}")]
    StopSimulationFailed(anyhow::Error),
    #[error("Failed to find component {0} in the simulation")]
    ComponentNotFound(String),
}

enum SimulatorState {
    Idle,
    Ready {
        builder: SimulationBuilder,
        topology: TopologySpec,
    },
    Running {
        simulation: Simulation,
        topology: TopologySpec,
    },
}

impl SimulatorState {
    fn to_status_update(&self) -> nest_api::StatusUpdate {
        match self {
            SimulatorState::Idle => nest_api::StatusUpdate::Idle,
            SimulatorState::Ready { topology, .. } => nest_api::StatusUpdate::Ready(
                nest_api::ReadyStatusUpdate { topology: topology.clone().into() }
            ),
            SimulatorState::Running { topology, .. } => nest_api::StatusUpdate::Running(
                nest_api::RunningStatusUpdate { topology: topology.clone().into() }
            ),
        }
    }
}

pub struct Simulator {
    state: SimulatorState,
}

impl Simulator {
    /// Create a new Simulator in the Idle state.
    pub fn new() -> Self {
        Self { state: SimulatorState::Idle }
    }

    /// Load a topology (Idle -> Ready).
    pub fn load_topology(&mut self, topology: TopologySpec) -> Result<(), SimulatorError> {
        if !matches!(self.state, SimulatorState::Idle) {
            return Err(SimulatorError::AlreadyReady);
        }
        let builder = SimulationBuilder::from_topology(&topology)
            .map_err(SimulatorError::LoadTopologyFailed)?;
        self.state = SimulatorState::Ready { builder, topology };
        Ok(())
    }

    /// Start the simulation (Ready -> Running).
    pub fn start(&mut self) -> Result<(), SimulatorError> {
        match mem::replace(&mut self.state, SimulatorState::Idle) {
            SimulatorState::Ready { builder, topology } => {
                let simulation = builder
                    .start_simulation()
                    .map_err(SimulatorError::StartSimulationFailed)?;
                self.state = SimulatorState::Running { simulation, topology };
                Ok(())
            }
            SimulatorState::Running { simulation, topology } => {
                self.state = SimulatorState::Running { simulation, topology }; // Restore state
                Err(SimulatorError::AlreadyRunning)
            }
            SimulatorState::Idle => Err(SimulatorError::NotReady),
        }
    }

    /// Stop the simulation (Running -> Ready).
    pub fn stop(&mut self) -> Result<(), SimulatorError> {
        match mem::replace(&mut self.state, SimulatorState::Idle) {
            SimulatorState::Running { simulation, topology } => {
                simulation.stop().map_err(SimulatorError::StopSimulationFailed)?;
                self.load_topology(topology)?;
                Ok(())
            }
            other => {
                self.state = other; // Restore state
                Err(SimulatorError::NotRunning)
            }
        }
    }

    /// Return a [SimulationReceiver] for transparently receiving logs from all components. Fails
    /// if the simulation is not running.
    /// 
    /// Note that multiple simulation log receivers will compete instead of each receiving all logs.
    pub fn log_receiver(&self) -> Result<SimulationReceiver<LogEntry>, SimulatorError> {
        match &self.state {
            SimulatorState::Running { simulation, .. } => Ok(simulation.log_receiver()),
            _ => {
                Err(SimulatorError::NotRunning)
            }
        }
    }

    /// Return a [SimulationReceiver] for transparently receiving events from all components. Fails
    /// if the simulation is not running.
    /// 
    /// Note that multiple simulation events receivers will compete instead of each receiving all logs.
    pub fn event_receiver(&self) -> Result<SimulationReceiver<ComponentEvent>, SimulatorError> {
        match &self.state {
            SimulatorState::Running { simulation, .. } => Ok(simulation.event_receiver()),
            _ => Err(SimulatorError::NotRunning),
        }
    }

    /// Return a reference to a [ComponentHandle] for the component matching the given name. Fails
    /// if the simulation is not running.
    pub fn component_handle(&self, name: &str) -> Result<&ComponentHandle, SimulatorError> {
        match &self.state {
            SimulatorState::Running { simulation, .. } => {
                simulation.handle(name)
                    .ok_or_else(|| SimulatorError::ComponentNotFound(name.into()))
            }
            _ => Err(SimulatorError::NotRunning),
        }
    }

    /// Return the current status of the simulator.
    pub fn status(&self) -> nest_api::StatusUpdate {
        self.state.to_status_update()
    }

    /// Return a reference to the currently loaded [Topology]. Fails if the simulation is not ready
    /// or running.
    pub fn topology(&self) -> Result<&TopologySpec, SimulatorError> {
        match &self.state {
            SimulatorState::Ready { topology, .. } => Ok(topology),
            SimulatorState::Running { topology, .. } => Ok(topology),
            _ => Err(SimulatorError::NoTopologyLoaded),
        }
    }

    /// Return the instant the simulation was started. Fails if the simulation is not running.
    pub fn sim_started_at(&self) -> Result<Instant, SimulatorError> {
        match &self.state {
            SimulatorState::Running { simulation, .. } => Ok(simulation.started_at()),
            _ => Err(SimulatorError::NotRunning),
        }
    }
}
