use std::time::Instant;

use super::{Component, ComponentHandle, ComponentHandleCommand, SystemPort};
use crate::{
    component::{
        ComponentEvent, ComponentParams, HandlerStatus, MessageDispatcher
    },
    ports::{BoolMessage, I2cRequest, I2cRequestOp, I2cResponse, I2cResponseOp, I2cTransaction, IntMessage, I2cMessage, TextMessage},
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
    i2c_port: SystemPort<I2cMessage>,
}

impl TestComponent {
    pub fn new(config: TestComponentConfig,
        cumsum_port: SystemPort<IntMessage>,
        print_port: SystemPort<TextMessage>,
        gpio0_port: SystemPort<BoolMessage>,
        gpio1_port: SystemPort<BoolMessage>,
        i2c_port: SystemPort<I2cMessage>) -> Self
    {
        Self { config, cumsum_port, print_port, gpio0_port, gpio1_port, i2c_port }
    }
}

impl Component for TestComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        // ComponentHandle::start creates a new handle for the provided closure and
        // takes care of spawning it as a new thread. It also passes fully initialized
        // command_receiver, logger and status instances to the closure.
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, status| {
            // Set initial status using the set_status macro
            set_status!(status.cumulative_sum, json!(0))?;

            // Create a new MessageDispatcher for registering message handlers
            let mut dispatcher = MessageDispatcher::<TestComponentState>::new();

            // Register a handler for the command_receiver. The callback function will be called each
            // time a message is received from that receiver, with the message and a custom mutable
            // object (state). Here, that state is of type TestComponentState.
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    ComponentHandleCommand::Stop => {
                        // Log infos with the logger
                        logger.info("Stopping component")?;
                        // To stop handling further messages, return HandlerStatus::Stop
                        return Ok(HandlerStatus::Stop);
                    }
                    ComponentHandleCommand::StartStatusUpdates => {
                        logger.info("Starting status updates")?;
                        // Update the state object. This modification will be visible from other
                        // closures since they all receive the object as argument.
                        state.update_status = true;
                    }
                    ComponentHandleCommand::StopStatusUpdates => {
                        logger.info("Stopping status updates")?;
                        state.update_status = false;
                    }
                    ComponentHandleCommand::SimulatorCommand { command, args } => {
                        let args_string = args.to_string();
                        let result = match command.as_str() {
                            "set_cumsum" => {
                                if let Ok(cumsum) = serde_json::from_value::<i32>(args) {
                                    state.current_sum = cumsum - self.config.initial_sum;
                                    Ok(())
                                } else {
                                    Err(format!("Bad arguments for simulator command {}: {}", command, args_string))
                                }
                            }
                            _ => Err(format!("Unknown simulator command: {}", command)),
                        };

                        match result {
                            Ok(()) => logger.info(format!("Received simulator command: {} {}", command, args_string))?,
                            Err(err) => logger.error(&err)?,
                        };
                    }
                }

                // If everything went well, return HandlerStatus::Ok
                Ok(HandlerStatus::Ok)
            }));

            // Register a handler for the cumsum port
            dispatcher.register_system_port(&self.cumsum_port, Box::new(|msg, state| {
                state.current_sum += msg;
                let cumsum = self.config.initial_sum + state.current_sum;

                if state.update_status {
                    // Update the cumulative_sum property of the status using the set_status macro.
                    // This will be visible from outside the component (e.g. displayed in a GUI).
                    set_status!(status.cumulative_sum, json!(cumsum))?;
                }
                logger.info(format!("New cumulative sum: {}", cumsum))?;

                // Send a message through the component's print port
                let output = format!("Cumulative sum: {}", cumsum);
                self.print_port.send(output);

                Ok(HandlerStatus::Ok)
            }));

            // Register a simple handler for the print port. Note that in general components
            // should not use println, this is just for demonstration purposes.
            dispatcher.register_system_port(&self.print_port, Box::new(|msg, _state| {
                println!("{} Got: {}", self.config.greeting, msg);
                // Send a simulation event with the received data
                event_sender.send(ComponentEvent::sim_event("print", msg))?;
                Ok(HandlerStatus::Ok)
            }));

            // Register gpio0 port handler
            dispatcher.register_system_port(&self.gpio0_port, Box::new(|msg, _state| {
                // Echo on gpio1
                self.gpio1_port.send(msg);

                Ok(HandlerStatus::Ok)
            }));

            // Register i2c port handler
            dispatcher.register_system_port(&self.i2c_port, Box::new(|msg, state| {
                let I2cTransaction::Request(I2cRequest { addr, op }) = msg
                else {
                    // Not a request: ignore
                    return Ok(HandlerStatus::Ok);
                };


                if addr != 0x42 {
                    return Ok(HandlerStatus::Ok)
                }

                let response = match op {
                    I2cRequestOp::Write(buf) => {
                        if buf.len() >= 1 {
                            state.current_sum += buf[0] as i32;
                        } else {
                            logger.warning("Received zero-length i2c write request")?
                        }
                        I2cTransaction::Response(I2cResponse::new(I2cResponseOp::Write))
                    }
                    I2cRequestOp::Read(len) => {
                        let mut buf = Vec::new();
                        if len >= 1 {
                            buf.push((self.config.initial_sum + state.current_sum) as u8);
                        } else {
                            logger.warning("Received zero-length i2c read request")?
                        }
                        I2cTransaction::Response(I2cResponse::new(I2cResponseOp::Read(buf)))
                    }
                };

                self.i2c_port.send(response);

                Ok(HandlerStatus::Ok)
            }));

            // Finally, start dispatching messages (blocks until HandlerStatus::Stop is returned by
            // a handler)
            dispatcher.start_dispatch(TestComponentState::default())?;
            Ok(())
        });

        // Return the handle to the calling thread
        Ok(handle)
    }

    fn from_params(params: ComponentParams) -> Result<Self> where Self: Sized {
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
                "i2c" => test_component.i2c_port = value.as_i2c()?.clone(),
                _ => bail!("Unknown system port {} for TestComponent", key),
            }
        }

        // Return the fully initialized component
        Ok(test_component)
    }
}
