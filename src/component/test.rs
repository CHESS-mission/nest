use anyhow::{bail, Result};
use serde_json::json;
use crate::{component::{ComponentParams, HandlerStatus, MessageDispatcher}, ports::{IntMessage, TextMessage}, set_status};
use super::{Component, ComponentMonitor, MonitorCommand, SystemPort};

// Config

#[derive(Clone, Debug)]
pub struct TestComponentConfig {
    pub initial_sum: i32,
    pub greeting: String,
}

impl Default for TestComponentConfig {
    fn default() -> Self {
        TestComponentConfig {
            initial_sum: 0,
            greeting: "Hello!".into(),
        }
    }
}

// State

#[derive(Clone, Debug)]
struct TestComponentState {
    pub current_sum: i32,
    pub update_status: bool,
}

impl Default for TestComponentState {
    fn default() -> Self {
        TestComponentState {
            current_sum: 0,
            update_status: true,
        }
    }
}

// Component

#[derive(Default)]
pub struct TestComponent {
    config: TestComponentConfig,
    cumsum_port: SystemPort<IntMessage>,
    print_port: SystemPort<TextMessage>,
}

impl TestComponent {
    pub fn new(config: TestComponentConfig, cumsum_port: SystemPort<IntMessage>, print_port: SystemPort<TextMessage>) -> Self {
        Self { config, cumsum_port, print_port }
    }
}

impl Component for TestComponent {
    fn start(self) -> Result<ComponentMonitor> {
        // ComponentMonitor::start_monitor creates a new monitor for the provided closure and
        // takes care of spawning it as a new thread. It also passes fully initialized
        // command_receiver, logger and status instances to the closure.
        let monitor = ComponentMonitor::start_monitor(move |command_receiver, logger, status| {
            // Set initial status using the set_status macro
            set_status!(status.cumulative_sum, json!(0))?;

            // Create a new MessageDispatcher for registering message handlers
            let mut dispatcher = MessageDispatcher::<TestComponentState>::new();

            // Register a handler for the command_receiver. The callback function will be called each
            // time a message is received from that receiver, with the message and a custom mutable
            // object (state). Here, that state is of type TestComponentState.
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    MonitorCommand::Stop => {
                        // Log infos with the logger
                        logger.info("Stopping component")?;
                        // To stop handling further messages, return HandlerStatus::Stop
                        return Ok(HandlerStatus::Stop);
                    }
                    MonitorCommand::StartStatusUpdates => {
                        logger.info("Starting status updates")?;
                        // Update the state object. This modification will be visible from other
                        // closures since they all receive the object as argument.
                        state.update_status = true;
                    }
                    MonitorCommand::StopStatusUpdates => {
                        logger.info("Stopping status updates")?;
                        state.update_status = false;
                    }
                }

                // If everything went well, return HandlerStatus::Ok
                Ok(HandlerStatus::Ok)
            }));

            // Register a handler for the cumsum port
            dispatcher.register_system_port(&self.cumsum_port, Box::new(|msg, state| {
                state.current_sum += msg.data;
                let cumsum = self.config.initial_sum + state.current_sum;

                if state.update_status {
                    // Update the cumulative_sum property of the status using the set_status macro.
                    // This will be visible from outside the component (e.g. displayed in a GUI).
                    set_status!(status.cumulative_sum, json!(cumsum))?;
                }
                logger.info(&format!("New cumulative sum: {}", cumsum))?;

                // Send a message through the component's print port
                let output = TextMessage::new(&format!("Cumulative sum: {}", cumsum));
                self.print_port.send(&output)?;

                Ok(HandlerStatus::Ok)
            }));

            // Register a simple handler for the print port. Note that in general components
            // should not use println, this is just for demonstration purposes.
            dispatcher.register_system_port(&self.print_port, Box::new(|msg, _state| {
                println!("{} Got: {}", self.config.greeting, msg.data);
                Ok(HandlerStatus::Ok)
            }));

            // Finally, start dispatching messages (blocks until HandlerStatus::Stop is returned by
            // a handler)
            dispatcher.start_dispatch(TestComponentState::default())?;
            Ok(())
        });

        // Return the monitor to the calling thread
        Ok(monitor)
    }

    fn from_params(params: &ComponentParams) -> Result<Self> where Self: Sized {
        let mut test_component = Self::default();

        // Parse config parameters into a TestComponentConfig
        for (key, value) in &params.config {
            match key.as_str() {
                "initial_sum" => test_component.config.initial_sum = value.parse()?,
                "greeting" => test_component.config.greeting = value.clone(),
                _ => bail!("Unknown config parameter {} for TestComponent", key),
            }
        }

        // Get system ports from params
        for (key, value) in &params.system_ports {
            match key.as_str() {
                "cumsum" => test_component.cumsum_port = value.as_int()?.clone(),
                "print" => test_component.print_port = value.as_text()?.clone(),
                _ => bail!("Unknown system port {} for TestComponent", key),
            }
        }

        // Return the fully initialized component
        Ok(test_component)
    }
}
