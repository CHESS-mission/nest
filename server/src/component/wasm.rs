use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use wasmtime::component::*;
use wasmtime::{Engine, Store};

use crate::component::wasm::host::{FromWit, HostCtx, IntoWit};
use crate::{component::{Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus, MessageDispatcher}, ports::AnySystemPort, set_status, with_message_wrapper};

mod host;

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmComponentConfig {
    /// Path or URL to the WASM module
    pub module: String,
}

// State

struct WasmComponentState {
    pub update_status: bool,
    pub store: Store<HostCtx>,
}

// Component

#[derive(Default)]
pub struct WasmComponent {
    config: WasmComponentConfig,
    ports: HashMap<String, AnySystemPort>,
}

impl WasmComponent {}

impl Component for WasmComponent {
    fn start(self: Box<Self>) -> Result<ComponentHandle> {
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, status| {
            // Initialize status
            set_status!(status.wasm, json!({
                "module": self.config.module,
                "ports": self.ports.keys().collect::<Vec<_>>(),
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
                        todo!();
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
            host::nest::plugin::log::add_to_linker::<HostCtx, HasSelf<_>>(&mut linker, |ctx| ctx)?;

            // Define imports
            for (name, any_port) in &self.ports {
                let import_name = format!("{}-out", name);
                let any_port = any_port.clone();
                with_message_wrapper!(AnySystemPort, any_port, |port| {
                    linker.root().func_wrap(&import_name, move |_store, (msg,)| {
                        port.send(M::from_wit(msg));
                        Ok(())
                    })?;
                });
            }

            // Instantiate the component
            let instance = linker.instantiate(&mut store, &component)?;

            // Register ports handlers
            for (name, any_port) in &self.ports {
                let logger = &logger;
                with_message_wrapper!(AnySystemPort, any_port, |port| {
                    let export_name = format!("{}-in", name);
                    let port_in = instance.get_typed_func::<(_,), ()>(&mut store, &export_name).expect("Export not found");

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
            logger.info("WASM component starting")?;
            dispatcher.start_dispatch(WasmComponentState { update_status: true, store })?;

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
            comp.ports.insert(k.clone(), v.clone());
        }

        Ok(comp)
    }
}
