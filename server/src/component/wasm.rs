use anyhow::Result;
use extism::{CurrentPlugin, Manifest, PTR, Plugin, PluginBuilder, UserData, Val, Wasm};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

use crate::{
    component::{ComponentParams, HandlerStatus, Logger, MessageDispatcher},
    ports::{
        AnySystemPort, BoolMessage, BytesMessage, FloatMessage, IntMessage, JsonMessage,
        TextMessage,
    },
    set_status, with_message_wrapper,
};

use super::{Component, ComponentHandle, ComponentHandleCommand};

// Ports output host function

#[derive(Clone)]
struct HostCtx {
    logger: Logger,
    port: AnySystemPort,
    import_name: String,
}

fn host_emit(
    plugin: &mut CurrentPlugin,
    vals: &[Val],
    _rets: &mut [Val],
    ctx: UserData<HostCtx>,
) -> std::result::Result<(), extism::Error> {
    let ctx_arc = ctx.get()?;
    let HostCtx {
        ref logger,
        ref port,
        ref import_name,
    } = *ctx_arc.lock().unwrap();

    match port {
        AnySystemPort::Int(port) => {
            if let Ok(msg) = plugin.memory_get_val(&vals[0]) {
                logger.info(format!("host env::{} -> \"{}\"", import_name, msg))?;
                port.send(IntMessage::new(msg));
            }
        }
        AnySystemPort::Float(port) => {
            if let Ok(msg) = plugin.memory_get_val(&vals[0]) {
                logger.info(format!("host env::{} -> \"{}\"", import_name, msg))?;
                port.send(FloatMessage::new(msg));
            }
        }
        AnySystemPort::Bool(port) => {
            if let Ok(msg) = plugin.memory_get_val(&vals[0]) {
                logger.info(format!("host env::{} -> \"{}\"", import_name, msg))?;
                port.send(BoolMessage::new(msg));
            }
        }
        AnySystemPort::Bytes(port) => {
            if let Ok(msg) = plugin.memory_get_val(&vals[0]) {
                logger.info(format!("host env::{} -> \"{:?}\"", import_name, msg))?;
                port.send(BytesMessage::new(msg));
            }
        }
        AnySystemPort::Text(port) => {
            if let Ok(msg) = plugin.memory_get_val::<&str>(&vals[0]) {
                logger.info(format!("host env::{} -> \"{}\"", import_name, msg))?;
                port.send(TextMessage::new(msg));
            }
        }
        AnySystemPort::Json(port) => {
            if let Ok(msg) = plugin.memory_get_val(&vals[0]) {
                logger.info(format!("host env::{} -> \"{}\"", import_name, msg))?;
                port.send(JsonMessage::new(msg));
            }
        }
    }

    Ok(())
}

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmComponentConfig {
    /// Path or URL to the WASM module (wasm.module)
    pub module: String,
}

// State used by the dispatcher

#[derive(Debug)]
struct WasmComponentState {
    pub plugin: Plugin,
    pub update_status: bool,
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
        let handle =
            ComponentHandle::start(move |command_receiver, _event_sender, logger, status| {
                // Initialize status
                set_status!(
                    status.wasm,
                    json!({
                        "module": self.config.module,
                        "ports": self.ports.keys().collect::<Vec<_>>(),
                    })
                )?;

                // Announce start
                logger.info("WASM component starting")?;

                // Build the Extism plugin (lives in state)
                let wasm_src = if self.config.module.starts_with("http://")
                    || self.config.module.starts_with("https://")
                {
                    Wasm::url(&self.config.module)
                } else {
                    Wasm::file(&self.config.module)
                };
                let manifest = Manifest::new([wasm_src]);

                let mut builder = PluginBuilder::new(manifest).with_wasi(true);
                // Register configured host imports: env::<function> routed to the mapped out port
                for (port_name, port) in &self.ports {
                    let import_name = format!("{}_out", port_name);
                    let host_ctx = HostCtx {
                        logger: logger.clone(),
                        port: port.clone(),
                        import_name: import_name.clone(),
                    };
                    builder = builder.with_function_in_namespace(
                        "env",
                        import_name,
                        [PTR],
                        [],
                        UserData::new(host_ctx),
                        host_emit,
                    );
                }

                let plugin = match builder.build() {
                    Ok(p) => p,
                    Err(e) => {
                        logger.error(format!("failed to build WASM plugin: {}", e))?;
                        return Ok(());
                    }
                };

                let mut dispatcher = MessageDispatcher::<WasmComponentState>::new();

                // Commands
                dispatcher.register(
                    command_receiver,
                    Box::new(|msg, state| {
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
                    }),
                );

                // Register ports handlers
                for (name, any_port) in &self.ports {
                    let export_name = format!("{}_in", name);
                    let logger = &logger;
                    let status = &status;
                    with_message_wrapper!(AnySystemPort, any_port, |port| {
                        dispatcher.register_system_port(
                            port,
                            Box::new(move |msg, state| {
                                logger.info(format!(
                                    "[{}] received input: \"{:?}\"",
                                    name, msg.data
                                ))?;

                                match state.plugin.call::<_, String>(&export_name, &msg.data) {
                                    Ok(result) => {
                                        if state.update_status {
                                            set_status!(
                                                status.last.name,
                                                json!({ "in": msg.data, "out": result })
                                            )?;
                                        }
                                        logger.info(format!(
                                            "export '{}' completed; out=\"{}\"",
                                            export_name, result
                                        ))?;
                                    }
                                    Err(e) => {
                                        logger.error(format!(
                                            "export '{}' failed: {}",
                                            export_name, e
                                        ))?;
                                    }
                                }
                                Ok(HandlerStatus::Ok)
                            }),
                        );
                    });
                }

                logger.info("WASM component ready")?;
                dispatcher.start_dispatch(WasmComponentState {
                    plugin,
                    update_status: true,
                })?;
                Ok(())
            });

        Ok(handle)
    }

    fn from_params(params: &ComponentParams) -> Result<Self>
    where
        Self: Sized,
    {
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
