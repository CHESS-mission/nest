use std::{mem, time::Instant};

use thiserror::Error;

use crate::{component::{ComponentEvent, ComponentHandle, LogEntry}, simulation::{Simulation, SimulationBuilder, SimulationReceiver, Topology}};

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
        topology: Topology,
    },
    Running {
        simulation: Simulation,
        started_time: Instant,
        topology: Topology,
    },
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
    pub fn load_topology(&mut self, topology: Topology) -> Result<(), SimulatorError> {
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
                let started_time = Instant::now();
                let simulation = builder
                    .start_simulation()
                    .map_err(SimulatorError::StartSimulationFailed)?;
                self.state = SimulatorState::Running { simulation, started_time, topology };
                Ok(())
            }
            SimulatorState::Running { simulation, started_time, topology } => {
                self.state = SimulatorState::Running { simulation, started_time, topology }; // Restore state
                Err(SimulatorError::AlreadyRunning)
            }
            SimulatorState::Idle => Err(SimulatorError::NotReady),
        }
    }

    /// Stop the simulation (Running -> Idle).
    pub fn stop(&mut self) -> Result<(), SimulatorError> {
        match mem::replace(&mut self.state, SimulatorState::Idle) {
            SimulatorState::Running { simulation, started_time: _, topology: _ } => {
                simulation.stop().map_err(SimulatorError::StopSimulationFailed)?;
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
            SimulatorState::Running { simulation, started_time: _, topology: _ } => Ok(simulation.log_receiver()),
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
            SimulatorState::Running { simulation, started_time: _, topology: _ } => Ok(simulation.event_receiver()),
            _ => Err(SimulatorError::NotRunning),
        }
    }

    /// Return a reference to a [ComponentHandle] for the component matching the given name. Fails
    /// if the simulation is not running.
    pub fn component_handle(&self, name: &str) -> Result<&ComponentHandle, SimulatorError> {
        match &self.state {
            SimulatorState::Running { simulation, started_time: _, topology: _ } => {
                simulation.handle(name)
                    .ok_or_else(|| SimulatorError::ComponentNotFound(name.into()))
            }
            _ => Err(SimulatorError::NotRunning),
        }
    }

    /// Return a reference to the currently loaded [Topology]. Fails if the simulation is not ready
    /// or running.
    pub fn topology(&self) -> Result<&Topology, SimulatorError> {
        match &self.state {
            SimulatorState::Ready { builder: _, topology } => Ok(topology),
            SimulatorState::Running { simulation: _, started_time: _, topology } => Ok(topology),
            _ => Err(SimulatorError::NoTopologyLoaded),
        }
    }

    /// Return the instant the simulation was started. Fails if the simulation is not running.
    pub fn started_time(&self) -> Result<Instant, SimulatorError> {
        match self.state {
            SimulatorState::Running { simulation: _, started_time, topology: _ } => Ok(started_time),
            _ => Err(SimulatorError::NotRunning),
        }
    }
}
