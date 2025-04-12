use std::{collections::HashMap, sync::{Arc, Mutex}, thread};

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Select, Sender};

use super::{BoolMessage, BytesMessage, Component, ComponentMonitor, ConnectablePort, FloatMessage, HasPortType, IntMessage, JsonMessage, ManagedSystemPort, MonitorCommand, PortQueueCapacity, SystemPort, TextMessage};

// Config

pub struct TestComponentConfig {
    pub initial_sum: i32,
    pub greeting: String,
}

// State

struct TestComponentState {
    pub current_sum: i32,
}

enum GenericSystemPort {
    Int(SystemPort<TestComponent, TestComponentState, IntMessage>),
    Float(SystemPort<TestComponent, TestComponentState, FloatMessage>),
    Bool(SystemPort<TestComponent, TestComponentState, BoolMessage>),
    Bytes(SystemPort<TestComponent, TestComponentState, BytesMessage>),
    Text(SystemPort<TestComponent, TestComponentState, TextMessage>),
    Json(SystemPort<TestComponent, TestComponentState, JsonMessage>),
}

// Component

pub struct TestComponent {
    ports: HashMap<String, GenericSystemPort>,
}

impl TestComponent {
    pub fn new(config: TestComponentConfig) -> Self {
        // Create ports
        let cumsum: SystemPort<TestComponent, TestComponentState, IntMessage> = SystemPort::new(
            Box::new(move |hub, state, msg| {
                state.current_sum += msg.data;
                let output = TextMessage::new(&format!("Cumulative sum: {}", config.initial_sum + state.current_sum));
                hub.port_by_name("print").unwrap().send(&output)?;
                Ok(())
            }),
            PortQueueCapacity::Unbounded);

        let print: SystemPort<TestComponent, TestComponentState, TextMessage> = SystemPort::new(
            Box::new(move |_hub, _state, msg| {
                println!("{} Got: {}", config.greeting, msg.data);
                Ok(())
            }),
            PortQueueCapacity::Unbounded);

        let mut ports = HashMap::new();

        ports.insert("cumsum".into(), GenericSystemPort::Int(cumsum));
        ports.insert("print".into(), GenericSystemPort::Text(print));

        Self { ports }
    }

    fn loop_recv_dispatch(&self, state: &mut TestComponentState, observable_state: &Arc<Mutex<String>>, monitor_rx: &Receiver<MonitorCommand>) -> Result<()> {
        let mut sel = Select::new();

        // Register monitor rx at index 0
        sel.recv(monitor_rx);

        // Register all ports
        let ports: Vec<_> = self.ports.values().collect();
        for port in &ports {
            Self::generic_to_managed(port).register(&mut sel);
        }

        let mut update_observable_state = false;

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();

            if index == 0 {
                // Handle monitor command
                match monitor_rx.recv()? {
                    MonitorCommand::Stop => return Ok(()),
                    MonitorCommand::StartObservableStateUpdates => update_observable_state = true,
                    MonitorCommand::StopObservableStateUpdates => update_observable_state = false,
                }
            } else {
                // Dispatch to port
                Self::generic_to_managed(&ports[index-1]).recv(self, state)?;
            }

            if update_observable_state {
                *observable_state.lock().unwrap() = format!("Current sum: {}", state.current_sum);
            }
        }
    }

    fn generic_to_managed(gen_port: &GenericSystemPort) -> &(dyn ManagedSystemPort<Self, TestComponentState>) {
        match gen_port {
            GenericSystemPort::Int(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
            GenericSystemPort::Float(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
            GenericSystemPort::Bool(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
            GenericSystemPort::Bytes(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
            GenericSystemPort::Text(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
            GenericSystemPort::Json(port) => port as &dyn ManagedSystemPort<Self, TestComponentState>,
        }
    }
}

impl Component for TestComponent {
    fn start(self) -> Result<ComponentMonitor> {
        let (tx, rx) = unbounded();
        let observable_state = Arc::new(Mutex::new(String::new()));
        let observable_state_clone = observable_state.clone();
        let handle = thread::spawn(move || {
            let mut state = TestComponentState { current_sum: 0 };
            self.loop_recv_dispatch(&mut state, &observable_state, &rx)
        });
        Ok(ComponentMonitor::new(handle, tx, observable_state_clone))
    }
}

macro_rules! impl_has_port_type {
    ( $enum_type:ident : $( $variant:ident => $msg_type:ty ),* $(,)? ) => {
        $(
            impl HasPortType<$msg_type> for TestComponent {
                fn port_by_name(&self, name: &str) -> Option<&dyn ConnectablePort<$msg_type>> {
                    self.ports.get(name).and_then(|v|
                        match v {
                            $enum_type::$variant(port) => Some(port as &dyn ConnectablePort<$msg_type>),
                            _ => None
                        }
                    )
                }

                fn port_by_name_mut(&mut self, name: &str) -> Option<&mut dyn ConnectablePort<$msg_type>> {
                    self.ports.get_mut(name).and_then(|v|
                        match v {
                            $enum_type::$variant(port) => Some(port as &mut dyn ConnectablePort<$msg_type>),
                            _ => None
                        }
                    )
                }
            }
        )*
    };
}

impl_has_port_type! {
    GenericSystemPort:
        Int => IntMessage,
        Float => FloatMessage,
        Bool => BoolMessage,
        Bytes => BytesMessage,
        Text => TextMessage,
        Json => JsonMessage,
}
