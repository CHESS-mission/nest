use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use wasmtime::component::*;
use wasmtime::{Engine, Store};

use crate::component::wasm::host::nest_component::NestComponent;
use crate::component::wasm::host::nest_component::nest::plugin::errors::{CommandError, InitError};
use crate::component::wasm::host::timeout_ext::TimeoutExt;
use crate::component::wasm::host::{
    FromWit, HostCtx, IntoWit,
};
use crate::component::wasm::timeout::start_timeout_scheduler;
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
mod timeout;

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
    fn start(self: Box<Self>, sim_start_time: Instant) -> Result<ComponentHandle> {
        let handle = ComponentHandle::start(move |command_receiver, event_sender, logger, status| {
            // Initialize status
            set_status!(status.wasm, json!({
                "module": self.config.module,
                "ports": self.sys_ports.keys().collect::<Vec<_>>(),
            }))?;

            // Start timeout scheduler
            let timeout_scheduler_handle = start_timeout_scheduler();

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

            let host_ctx = HostCtx::new(
                logger.clone(),
                event_sender.clone(),
                sim_start_time,
                timeout_scheduler_handle.timeout_request_tx.clone()
            );
            let mut store = Store::new(&engine, host_ctx);
            let mut linker = Linker::new(&engine);
            linker.allow_shadowing(true);

            // Make imports trap by default, and replace the traps of imports
            // which NEST cannot implement by ones naming the import.
            linker.define_unknown_imports_as_traps(&component)?;
            define_named_import_traps(&engine, &mut linker, &component);

            // Add host WIT interfaces to linker
            NestComponent::add_to_linker::<HostCtx, HasSelf<_>>(&mut linker, |ctx| ctx)?;
            TimeoutExt::add_to_linker::<HostCtx, HasSelf<_>>(&mut linker, |ctx| ctx)?;

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
                            logger.error(format!("Failed to call export {}: {:#}", export_name, e))?;
                        }
                        if let Err(e) = port_in.post_return(&mut state.store) {
                            logger.error(format!("Post return failed for {}: {:#}", export_name, e))?;
                        }

                        Ok(HandlerStatus::Ok)
                    }));
                });
            }

            // Register timeout handler
            if let Ok(on_timeout) = instance.get_typed_func::<(_,), ()>(&mut store, "on-timeout") {
                let logger = &logger;
                dispatcher.register(timeout_scheduler_handle.timeout_rx.clone(), Box::new(move |msg, state| {
                    if let Err(e) = on_timeout.call(&mut state.store, (msg.id,)) {
                        logger.error(format!("Failed to call export on-timeout: {:#}", e))?;
                    }
                    if let Err(e) = on_timeout.post_return(&mut state.store) {
                        logger.error(format!("Post return failed for on-timeout: {:#}", e))?;
                    }

                    Ok(HandlerStatus::Ok)
                }));
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

            // Stop timeout scheduler
            timeout_scheduler_handle.stop_and_join();

            Ok(())
        });

        Ok(handle)
    }

    fn from_params(params: ComponentParams) -> Result<Self> where Self: Sized {
        // Parse config parameters into a TestComponentConfig
        let mut comp = Self {
            config: serde_json::from_value::<WasmComponentConfig>(params.config().clone())?,
            ..Default::default()
        };

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

// Import traps

/// Return whether an imported item can be replaced by traps naming it
fn import_is_stubable(engine: &Engine, item: &wasmtime::component::types::ComponentItem) -> bool {
    use wasmtime::component::types::ComponentItem as Item;
    match item {
        Item::ComponentFunc(_) | Item::Resource(_) | Item::Type(_) => true,
        Item::ComponentInstance(i) => i
            .exports(engine)
            .all(|(_, item)| import_is_stubable(engine, &item)),
        _ => false,
    }
}

/// Replace the traps of imports which NEST does not implement by traps which
/// name the import they stand for, and say why it is not implemented.
fn define_named_import_traps<T: Send + Sync + 'static>(
    engine: &Engine,
    linker: &mut Linker<T>,
    component: &wasmtime::component::Component,
) {
    use wasmtime::component::types::ComponentItem as Item;

    fn stub<'a, T: Send + Sync + 'a>(
        engine: &Engine,
        linker: &mut LinkerInstance<'a, T>,
        name: &str,
        item: &Item,
        path: String,
    ) {
        match item {
            Item::ComponentFunc(_) => {
                let _ = linker.func_new(name, move |_, _, _| {
                    anyhow::bail!(
                        "{path} is not implemented by the NEST host: components can only call the \
                         interfaces of the nest:plugin package, and imports of other interfaces, \
                         such as WASI, always trap"
                    )
                });
            }
            Item::Resource(_) => {
                // Resources cannot be trapped, they are only ever dropped
                let _ = linker.resource(name, ResourceType::host::<()>(), |_, _| Ok(()));
            }
            Item::ComponentInstance(i) => {
                let exports: Vec<_> = i.exports(engine).collect();
                if let Ok(mut instance) = linker.instance(name) {
                    for (name, item) in exports {
                        stub(engine, &mut instance, name, &item, format!("{path}#{name}"));
                    }
                }
            }
            _ => (),
        }
    }

    for (name, item) in component.component_type().imports(engine) {
        // Interfaces implemented by the NEST host, and port and environment
        // imports, which are defined below
        if name.starts_with("nest:") || ["emit", "get-time", "set-timer"].contains(&name) {
            continue;
        }
        if import_is_stubable(engine, &item) {
            stub(engine, &mut linker.root(), name, &item, name.to_string());
        }
    }
}
