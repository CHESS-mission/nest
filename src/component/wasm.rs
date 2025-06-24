use anyhow::Result;
use extism::{Manifest, Plugin, Wasm, PluginBuilder, UserData, PTR, host_fn};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
};
use extism::ValType;

use crossbeam_channel::{unbounded, Receiver, Select};

use super::{BoolMessage, BytesMessage, Component, ComponentMonitor, ConnectablePort, FloatMessage, HasPortType, IntMessage, JsonMessage, ManagedSystemPort, MonitorCommand, PortQueueCapacity, SystemPort, TextMessage};

// ---------------------------------------------------------------------------
// NEST port description helpers
// ---------------------------------------------------------------------------
#[derive(Clone)]
pub enum MessageType {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    Json,
}

pub struct InputPort {
    pub name: String,
    pub msg_type: MessageType,
}

pub struct OutputPort {
    pub name: String,
    pub msg_type: MessageType,
}

// Context passed to the common host function so it knows which out port triggered the call
struct OutCtx {
    port_name: String,
}

// Convert MessageType NEST to ValType Extism
fn message_type_to_valtype(msg_type: &MessageType) -> ValType {
    match msg_type {
        MessageType::Int => ValType::I64,
        MessageType::Float => ValType::F64,
        MessageType::Bool => ValType::I64,
        MessageType::Bytes => ValType::I64,
        MessageType::Text => ValType::I64,
        MessageType::Json => ValType::I64,
    }
}

fn common_out(
    plugin: &mut extism::CurrentPlugin,
    vals: &[extism::Val],
    rets: &mut [extism::Val],
    ctx: UserData<OutCtx>,
) -> Result<(), extism::Error> {
    let name = ctx.get()?.lock().unwrap().port_name.clone();

    if vals.is_empty() {
        println!("[wasm] OUT {}: <no argument>", name);
        return Ok(());
    }

    let offset = match vals[0] {
        extism::Val::I64(n) => n as u64,
        _ => {
            println!("[wasm] OUT {}: <unexpected argument type>", name);
            return Ok(());
        }
    };

    if let Some(handle) = plugin.memory_handle(offset) {
        // Try reading as a string
        match plugin.memory_str(handle) {
            Ok(s) => println!("[wasm] OUT {}: \"{}\"", name, s),
            Err(_) => {
                // Fallback: try as bytes
                match plugin.memory_bytes(handle) {
                    Ok(bytes) => println!("[wasm] OUT {}: {:?}", name, bytes),
                    Err(e) => println!("[wasm] OUT {}: <failed to read memory: {}>", name, e),
                }
            }
        }
    } else {
        println!("[wasm] OUT {}: <invalid offset {}>", name, offset);
    }

    // Set return value to 0 to indicate success
    if !rets.is_empty() {
        rets[0] = extism::Val::I64(0);
    }
    Ok(())
}

// Config

pub struct WasmComponentConfig {
    pub name: String,
    pub wasm_url: String,
    pub input_ports: Vec<InputPort>,
    pub output_ports: Vec<OutputPort>,
}

// State
struct WasmComponentState;


enum GenericSystemPort {
    Int(SystemPort<WasmComponent, WasmComponentState, IntMessage>),
    Float(SystemPort<WasmComponent, WasmComponentState, FloatMessage>),
    Bool(SystemPort<WasmComponent, WasmComponentState, BoolMessage>),
    Bytes(SystemPort<WasmComponent, WasmComponentState, BytesMessage>),
    Text(SystemPort<WasmComponent, WasmComponentState, TextMessage>),
    Json(SystemPort<WasmComponent, WasmComponentState, JsonMessage>),
}

// Component

pub struct WasmComponent {
    plugin: Arc<Mutex<Plugin>>,
    ports: HashMap<String, GenericSystemPort>,
}

impl WasmComponent {
    // Creates a new WASM component from a URL
    pub fn new(config: WasmComponentConfig) -> Self {
        let url = if config.wasm_url.starts_with("http") {
            Wasm::url(&config.wasm_url)
        } else {
            Wasm::file(&config.wasm_url)
        };
        let manifest = Manifest::new([url]);

        let mut builder = PluginBuilder::new(manifest).with_wasi(true);

        // Register one host‑function per out port, all use same callback
        for op in &config.output_ports {
            let valtype = message_type_to_valtype(&op.msg_type);
            builder = builder.with_function_in_namespace(
                "env",
                &op.name,
                [valtype.clone()],
                [valtype],
                UserData::new(OutCtx { port_name: op.name.clone() }),
                common_out,
            );
        }

        let plugin = Arc::new(Mutex::new(builder.build().unwrap())); // mettre dans le state pour éviter le mutex !!!

        let mut ports = HashMap::new();

        // INPUTS: one SystemPort per in export function from plugin
        for ip in &config.input_ports {
            let fn_name = format!("{}", ip.name);
            let label   = ip.name.clone();
            let plg     = plugin.clone();

            // only handle TextMessage here
            let port = SystemPort::new(
                Box::new(move |comp: &WasmComponent, _st, msg: &TextMessage| {
                    let mut p = plg.lock().unwrap();
                    let res = p.call::<&str, &str>(&fn_name, &msg.data)?;
                    println!("[wasm] {} → {}", label, res);

                    // If a SystemPort with the same name exists on the Rust side, forward
                    if let Some(out) = comp.port_by_name(&label) {
                        out.send(&TextMessage::new(res))?; // let component respond when it wants
                    }
                    Ok(())
                }),
                PortQueueCapacity::Unbounded,
            );

            ports.insert(ip.name.clone(), GenericSystemPort::Text(port));
        }

        // OUTPUTS
        for op in &config.output_ports {
            let lbl = op.name.clone();
            let port = SystemPort::new(
                Box::new(move |_comp, _st, msg: &TextMessage| {
                            println!("[sim] {} OUT (loopback): {}", lbl, msg.data);
                            Ok(())
                        }),
                PortQueueCapacity::Unbounded,
            );
            ports.insert(op.name.clone(), GenericSystemPort::Text(port));
        }

        Self { plugin, ports }
    }


    fn loop_recv_dispatch(&self, state: &mut WasmComponentState, observable_state: &Arc<Mutex<String>>, monitor_rx: &Receiver<MonitorCommand>) -> Result<()> {
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
                Self::generic_to_managed(&ports[index - 1]).recv(self, state)?;
            }

            if update_observable_state {
                *observable_state.lock().unwrap() = "running".into();
            }
        }
    }

    fn generic_to_managed(gen_port: &GenericSystemPort) -> &(dyn ManagedSystemPort<Self, WasmComponentState>) {
        match gen_port {
            GenericSystemPort::Int(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
            GenericSystemPort::Float(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
            GenericSystemPort::Bool(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
            GenericSystemPort::Bytes(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
            GenericSystemPort::Text(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
            GenericSystemPort::Json(port) => port as &dyn ManagedSystemPort<Self, WasmComponentState>,
        }
    }
}


impl Component for WasmComponent {
    fn start(self) -> Result<ComponentMonitor> {
        let (tx, rx) = unbounded();
        let observable_state = Arc::new(Mutex::new(String::new()));
        let observable_state_clone = observable_state.clone();
        let handle = thread::spawn(move || {
            let mut state = WasmComponentState { };
            self.loop_recv_dispatch(&mut state, &observable_state, &rx)
        });
        Ok(ComponentMonitor::new(handle, tx, observable_state_clone))
    }
}

macro_rules! impl_has_port_type {
    ( $enum_type:ident : $( $variant:ident => $msg_type:ty ),* $(,)? ) => {
        $(
            impl HasPortType<$msg_type> for WasmComponent {
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