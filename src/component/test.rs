use super::{Component, ComponentMonitor, MonitorCommand, SystemPort};
use crate::{
    component::{
        qemu::{I2cOperation, I2cRequest, I2cResponse}, ComponentParams, HandlerStatus, MessageDispatcher
    },
    ports::{BoolMessage, IntMessage, JsonMessage, TextMessage},
    set_status,
};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::json;

// Config

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    gpio0_port: SystemPort<BoolMessage>,
    gpio1_port: SystemPort<BoolMessage>,
    i2c_port: SystemPort<JsonMessage>,
}

impl TestComponent {
    pub fn new(config: TestComponentConfig,
        cumsum_port: SystemPort<IntMessage>,
        print_port: SystemPort<TextMessage>,
        gpio0_port: SystemPort<BoolMessage>,
        gpio1_port: SystemPort<BoolMessage>,
        i2c_port: SystemPort<JsonMessage>) -> Self
    {
        Self { config, cumsum_port, print_port, gpio0_port, gpio1_port, i2c_port }
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

            // Register gpio0 port handler
            dispatcher.register_system_port(&self.gpio0_port, Box::new(|msg, _state| {
                // Echo on gpio1
                self.gpio1_port.send(&msg)?;

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c port handler
            dispatcher.register_system_port(&self.i2c_port, Box::new(|msg, state| {
                let request: I2cRequest = serde_json::from_value(msg.data).unwrap();

                if request.addr != 0x42 {
                    return Ok(HandlerStatus::Ok)
                }

                let response = match request.op {
                    I2cOperation::Write(buf) => {
                        if buf.len() >= 1 {
                            state.current_sum += buf[0] as i32;
                        } else {
                            logger.warning("Received zero-length i2c write request")?
                        }
                        serde_json::to_value(I2cResponse::Write)?
                    }
                    I2cOperation::Read(len) => {
                        let mut buf = Vec::new();
                        if len >= 1 {
                            buf.push((self.config.initial_sum + state.current_sum) as u8);
                        } else {
                            logger.warning("Received zero-length i2c read request")?
                        }
                        serde_json::to_value(I2cResponse::Read(buf))?
                    }
                };

                self.i2c_port.send(&JsonMessage::new(response))?;

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
        test_component.config = serde_json::from_value::<TestComponentConfig>(params.config().clone())?;

        // Get system ports from params
        for (key, value) in &params.system_ports {
            match key.as_str() {
                "cumsum" => test_component.cumsum_port = value.as_int()?.clone(),
                "print" => test_component.print_port = value.as_text()?.clone(),
                "gpio0" => test_component.gpio0_port = value.as_bool()?.clone(),
                "gpio1" => test_component.gpio1_port = value.as_bool()?.clone(),
                "i2c" => test_component.i2c_port = value.as_json()?.clone(),
                _ => bail!("Unknown system port {} for TestComponent", key),
            }
        }

        // Return the fully initialized component
        Ok(test_component)
    }
}
