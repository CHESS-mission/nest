use anyhow::{bail, Result};
use std::collections::HashMap;
use serde_json::json;
use extism::{Manifest, Plugin, PluginBuilder, Wasm, ValType, CurrentPlugin, Val, UserData};

use crate::{
    component::{ComponentParams, HandlerStatus, MessageDispatcher, Logger},
    ports::{SystemPort, TextMessage},
    set_status,
};

use super::{Component, ComponentMonitor, MonitorCommand};

// Config

#[derive(Clone, Debug, Default)]
pub struct WasmComponentConfig {
    /// Path or URL to the WASM module (wasm.module)
    pub module: String,
    /// Multi-port export mapping (plugin perspective):
    /// - "exports" are functions implemented and exposed by the plugin (wasm module)
    /// - the host calls these when receiving messages on input ports
    /// - config: wasm.exports.<in_port> = <export_fn>
    /// - example: wasm.exports.cmd = handle_command  => host calls plugin export "handle_command" on messages arriving on port "cmd"
    pub exports: HashMap<String, String>,
    /// Multi-port import mapping (plugin perspective):
    /// - "imports" are functions the plugin expects the host to provide (env::<name>)
    /// - when the plugin calls such an import, the host implementation emits a Text message on the mapped output port
    /// - config: wasm.imports.<out_port> = <import_fn>
    /// - example: wasm.imports.log = log  => host exposes env::log and forwards calls to output port "log"
    pub imports: HashMap<String, String>,
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
    in_ports: HashMap<String, SystemPort<TextMessage>>,   // input ports (mapped by wasm.exports.*)
    out_ports: HashMap<String, SystemPort<TextMessage>>,  // output ports (mapped by wasm.imports.*)
}

impl WasmComponent {}

impl Component for WasmComponent {
    fn start(self) -> Result<ComponentMonitor> {
    let monitor = ComponentMonitor::start_monitor(move |command_receiver, logger, status| {
            // Initialize status
            set_status!(status.wasm, json!({
                "module": self.config.module,
                "exports": self.config.exports,
                "imports": self.config.imports,
            }))?;

            // Announce start
            logger.info("WASM component starting")?;

            // Build the Extism plugin (lives in state)
            let wasm_src = if self.config.module.starts_with("http://") || self.config.module.starts_with("https://") {
                Wasm::url(&self.config.module)
            } else {
                Wasm::file(&self.config.module)
            };
            let manifest = Manifest::new([wasm_src]);
            // Host import: expose env::<name> (plugin import) that emits Text to the mapped out_port
            #[derive(Clone)]
            struct HostCtx { logger: Logger, out_port: SystemPort<TextMessage>, import_name: String }

            fn host_emit(plugin: &mut CurrentPlugin, vals: &[Val], rets: &mut [Val], ctx: UserData<HostCtx>) -> std::result::Result<(), extism::Error> {
                let ctx_arc = ctx.get()?;
                let ctx_guard = ctx_arc.lock().unwrap();
                let logger = ctx_guard.logger.clone();
                let out_port = ctx_guard.out_port.clone();
                let import = ctx_guard.import_name.clone();
                // Expect a single I64 argument: offset to memory (string)
                let s = if let Some(Val::I64(off)) = vals.get(0) {
                    if let Some(handle) = plugin.memory_handle(*off as u64) {
                        plugin.memory_str(handle).unwrap_or_default().to_string()
                    } else { String::new() }
                } else { String::new() };
                // Log and forward to out_port
                let _ = logger.info(&format!("host env::{} -> \"{}\"", import, s));
                let _ = out_port.send(&TextMessage::new(&s));
                if let Some(r0) = rets.get_mut(0) { *r0 = Val::I64(0); }
                Ok(())
            }

            let mut builder = PluginBuilder::new(manifest).with_wasi(true);
            // Register configured host imports: env::<function> routed to the mapped out port
            for (out_port_name, import_fn) in &self.config.imports {
                if let Some(out_port) = self.out_ports.get(out_port_name) {
                    let host_ctx = HostCtx { logger: logger.clone(), out_port: out_port.clone(), import_name: import_fn.clone() };
                    builder = builder.with_function_in_namespace("env", import_fn, [ValType::I64], [ValType::I64], UserData::new(host_ctx), host_emit);
                } else {
                    logger.warning(&format!("configured import '{}' has no matching out port '{}'", import_fn, out_port_name))?;
                }
            }

            let plugin = match builder.build() {
                Ok(p) => p,
                Err(e) => {
                    logger.error(&format!("failed to build WASM plugin: {}", e))?;
                    return Ok(());
                }
            };

            let mut dispatcher = MessageDispatcher::<WasmComponentState>::new();

            // Commands
            dispatcher.register(command_receiver, Box::new(|msg, state| {
                match msg {
                    MonitorCommand::Stop => {
                        logger.info("Stopping WASM component")?;
                        return Ok(HandlerStatus::Stop);
                    }
                    MonitorCommand::StartStatusUpdates => {
                        logger.info("Starting status updates")?;
                        state.update_status = true;
                    }
                    MonitorCommand::StopStatusUpdates => {
                        logger.info("Stopping status updates")?;
                        state.update_status = false;
                    }
                }
                Ok(HandlerStatus::Ok)
            }));

            // Resolve export mapping (multi-port)
            let mut exports_map: HashMap<String, String> = self.config.exports.clone();

            // Register input handlers for each mapped in port
            for (in_name, export_fn) in exports_map.drain() {
                if let Some(in_port) = self.in_ports.get(&in_name) {
                    let logger_in = logger.clone();
                    let export_name_clone = export_fn.clone();
                    let in_name_clone = in_name.clone();
                    let status_for_in = status.clone();
                    dispatcher.register_system_port(in_port, Box::new(move |msg, state| {
                        logger_in.info(&format!("[{}] received input: \"{}\"", in_name_clone, msg.data))?;

                        match state.plugin.call::<&str, String>(&export_name_clone, &msg.data) {
                            Ok(result) => {
                                if state.update_status {
                                    if let Ok(mut s) = status_for_in.lock() {
                                        let keys = vec!["last", in_name_clone.as_str()];
                                        let v = crate::macros::_get_or_create_json_path_mut(&mut *s, &keys);
                                        *v = json!({ "in": msg.data, "out": result });
                                    }
                                }
                                logger_in.info(&format!("export '{}' completed; out=\"{}\"", export_name_clone, result))?;
                            }
                            Err(e) => { logger_in.error(&format!("export '{}' failed: {}", export_name_clone, e))?; }
                        }
                        Ok(HandlerStatus::Ok)
                    }));
                } else {
                    logger.warning(&format!("configured export '{}' has no matching in port '{}'", export_fn, in_name))?;
                }
            }

            // Also log content of each declared port when used for outputs
            for (out_name, out_port) in self.out_ports.iter() {
                let logger_out = logger.clone();
                let out_name_clone = out_name.clone();
                dispatcher.register_system_port(out_port, Box::new(move |msg, _state| {
                    logger_out.info(&format!("[{}] wasm out -> {}", out_name_clone, msg.data))?;
                    Ok(HandlerStatus::Ok)
                }));
            }

            logger.info("WASM component ready")?;
            dispatcher.start_dispatch(WasmComponentState { plugin, update_status: true })?;
            Ok(())
        });

        Ok(monitor)
    }

    fn from_params(params: &ComponentParams) -> Result<Self> where Self: Sized {
        let mut comp = Self::default();

    // Config (multi-port)
        for (k, v) in &params.config {
            if k == "wasm.module" { comp.config.module = v.clone(); continue; }
            if let Some(p) = k.strip_prefix("wasm.exports.") { comp.config.exports.insert(p.to_string(), v.clone()); continue; }
            if let Some(p) = k.strip_prefix("wasm.imports.") { comp.config.imports.insert(p.to_string(), v.clone()); continue; }
            bail!("Unknown config parameter {} for WasmComponent", k);
        }

        // Ports: populate input and output maps based on configured mappings
        // Inputs
        for in_name in comp.config.exports.keys() {
            let gp = params.system_port(in_name).ok_or_else(|| anyhow::anyhow!(format!("Missing system port '{}' for wasm export", in_name)))?;
            comp.in_ports.insert(in_name.clone(), gp.as_text()?.clone());
        }

        // Outputs
        for out_name in comp.config.imports.keys() {
            let gp = params.system_port(out_name).ok_or_else(|| anyhow::anyhow!(format!("Missing system port '{}' for wasm import", out_name)))?;
            comp.out_ports.insert(out_name.clone(), gp.as_text()?.clone());
        }

    if comp.config.module.is_empty() { bail!("Missing required config 'wasm.module'"); }
    if comp.config.exports.is_empty() { bail!("Missing export mapping: set 'wasm.exports.<in>=<fn>'"); }

        Ok(comp)
    }
}
