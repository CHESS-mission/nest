use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use wasmtime::component::*;
use wasmtime::{Engine, Store};

use crate::component::wasm::host::{
    CommandError, FromWit, HostCtx, InitError, IntoWit, NestComponent,
};
use crate::ports::AnyEnvironmentPort;
use crate::with_env_var_wrapper;
use crate::{
    component::{
        Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
        MessageDispatcher,
    },
    ports::AnySystemPort,
    set_status, with_message_wrapper,
};

mod host;

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmComponentConfig {
    /// Path or URL to the WASM module
    pub module: String,
    #[serde(default)]
    pub options: Vec<(String, String)>,
}

// State

struct WasmComponentState {
    pub update_status: bool,
    pub store: Store<HostCtx>,
    pub nest_component: NestComponent,
}

// Component

#[derive(Default)]
pub struct WasmComponent {
    config: WasmComponentConfig,
    sys_ports: HashMap<String, AnySystemPort>,
    env_ports: HashMap<String, AnyEnvironmentPort>,
}

impl WasmComponent {}

impl Component for WasmComponent {
    fn start(self: Box<Self>) -> Result<ComponentHandle> {
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, status| {
            // Initialize status
            set_status!(status.wasm, json!({
                "module": self.config.module,
                "ports": self.sys_ports.keys().collect::<Vec<_>>(),
            }))?;

            let mut dispatcher = MessageDispatcher::<WasmComponentState>::new();

            // Commands
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    ComponentHandleCommand::Stop => {
                        logger.info("Stopping WASM component")?;
                        return Ok(HandlerStatus::Stop);
                    }
                    ComponentHandleCommand::StartStatusUpdates => {
                        logger.info("Starting status updates")?;
                        state.update_status = true;
                    }
                    ComponentHandleCommand::StopStatusUpdates => {
                        logger.info("Stopping status updates")?;
                        state.update_status = false;
                    }
                    ComponentHandleCommand::SimulatorCommand { command, args } => {
                        if let Err(error) = state.nest_component.call_on_command(&mut state.store, &command, &args.to_string())? {
                            match error {
                                CommandError::UnknownCommand => logger.error(format!("Unknown command: {}", command))?,
                                CommandError::InvalidValue(err_msg) => logger.error(format!("Invalid value for command {}: {}", command, err_msg))?,
                            }
                        }
                    }
                }
                Ok(HandlerStatus::Ok)
            }));

            // Build the Wasmtime instance
            let engine = Engine::default();
            // TODO: add URL support
            let component = wasmtime::component::Component::from_file(&engine, self.config.module)?;

            let host_ctx = HostCtx::new(logger.clone(), event_sender.clone());
            let mut store = Store::new(&engine, host_ctx);
            let mut linker = Linker::new(&engine);

            // Make imports trap by default
            linker.define_unknown_imports_as_traps(&component)?;
            linker.allow_shadowing(true);

            // Add host WIT interfaces to linker
            NestComponent::add_to_linker::<HostCtx, HasSelf<_>>(&mut linker, |ctx| ctx)?;

            // Define system ports imports
            for (name, any_port) in &self.sys_ports {
                let import_name = format!("{}-out", name);
                let any_port = any_port.clone();
                with_message_wrapper!(AnySystemPort, any_port, |port| {
                    linker.root().func_wrap(&import_name, move |_store, (msg,)| {
                        port.send(M::from_wit(msg));
                        Ok(())
                    })?;
                });
            }

            // Define environment ports imports
            for (name, any_port) in &self.env_ports {
                let import_name = format!("read-{}", name);
                let any_port = any_port.clone();
                with_env_var_wrapper!(AnyEnvironmentPort, any_port, |port| {
                    linker.root().func_wrap(&import_name, move |_store, ()| {
                        Ok((port.value().into_wit(),))
                    })?;
                });
            }

            // Instantiate the component
            let instance = linker.instantiate(&mut store, &component)?;
            let nest_component = NestComponent::new(&mut store, &instance)?;

            // Register ports handlers
            for (name, any_port) in &self.sys_ports {
                let logger = &logger;
                with_message_wrapper!(AnySystemPort, any_port, |port| {
                    let export_name = format!("{}-in", name);
                    let port_in = instance.get_typed_func::<(_,), ()>(&mut store, &export_name)?;

                    dispatcher.register_system_port(port, Box::new(move |msg, state| {
                        if let Err(e) = port_in.call(&mut state.store, (msg.into_wit(),)) {
                            logger.error(format!("Failed to call export {}: {}", &export_name, e))?;
                        }
                        if let Err(e) = port_in.post_return(&mut state.store) {
                            logger.error(format!("Post return failed for {}: {}", &export_name, e))?;
                        }

                        Ok(HandlerStatus::Ok)
                    }));
                });
            }

            // Starting
            logger.info("Initializing component...")?;
            if let Err(error) = nest_component.call_init(&mut store, &self.config.options)? {
                match error {
                    InitError::UnknownOption(err_msg) => logger.error(format!("Unknown option: {}", err_msg))?,
                    InitError::InvalidValue(err_msg) => logger.error(format!("Invalid value: {}", err_msg))?,
                }
            }
            logger.info("Initialization complete")?;
            dispatcher.start_dispatch(WasmComponentState { update_status: true, store, nest_component })?;

            Ok(())
        });

        Ok(handle)
    }

    fn from_params(params: &ComponentParams) -> Result<Self> where Self: Sized {
        let mut comp = Self::default();

        // Parse config parameters into a TestComponentConfig
        comp.config = serde_json::from_value::<WasmComponentConfig>(params.config().clone())?;

        // Get system ports from params
        for (k, v) in &params.system_ports {
            comp.sys_ports.insert(k.clone(), v.clone());
        }

        // Get environment ports from params
        for (k, v) in &params.env_ports {
            comp.env_ports.insert(k.clone(), v.clone());
        }

        Ok(comp)
    }
}
