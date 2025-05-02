use anyhow::Result;
use extism::{Manifest, Plugin, Wasm, PluginBuilder, CurrentPlugin, UserData, Val, PTR};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
};

use crossbeam_channel::{unbounded, Receiver, Select};

use super::{BoolMessage, BytesMessage, Component, ComponentMonitor, ConnectablePort, FloatMessage, HasPortType, IntMessage, JsonMessage, ManagedSystemPort, MonitorCommand, PortQueueCapacity, SystemPort, TextMessage};

// Config

pub type HostCallback = fn(&mut CurrentPlugin, &[Val], &mut [Val], UserData<()>) -> Result<(), extism::Error>; // type returned by hostfn! macro

pub struct HostFunction { // wrap host callback with import name expected by WASM module
    pub name: &'static str, //required because host_fn! does not embed WASM module name
    pub callback: HostCallback,
}

pub struct WasmComponentConfig {
    pub name: String,
    pub wasm_url: String,
    pub export_func: String, // function to call inside the WASM module
    pub host_functions: Vec<HostFunction> // host functions defined outside the WASM module, imported into the module
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
        // Load the WASM module
        let name = config.name.clone();
        let export_fn = config.export_func.clone();
        let url = if config.wasm_url.starts_with("http://") || config.wasm_url.starts_with("https://") {
            Wasm::url(&config.wasm_url)
        } else {
            Wasm::file(&config.wasm_url)
        };
        let manifest = Manifest::new([url]);

        let mut builder = PluginBuilder::new(manifest).with_wasi(true);

        // Add host functions to the plugin
        for hf in &config.host_functions {
            builder = builder.with_function_in_namespace(
                "env",
                hf.name,
                [PTR],
                [PTR],
                UserData::new(()),
                hf.callback,
            );
        }

        let plugin = Arc::new(Mutex::new(builder.build().unwrap()));

        // Input port: sends data to the plugin and forwards the result to the output port
        let plugin_in: Arc<Mutex<Plugin>> = Arc::clone(&plugin);
        let name_in = name.to_string();
        let input_port: SystemPort<WasmComponent, WasmComponentState, TextMessage> =
            SystemPort::new(
                Box::new(move |hub, _state, msg| {
                    // Call the plugin
                    let mut plugin = plugin_in.lock().unwrap();
                    let res = plugin.call::<&str, &str>(&export_fn, &msg.data)?;
                    println!("{} [wasm] plugin returned: {}", name_in, res);

                    // Forward the response to output port if connected
                    if let Some(out) = hub.port_by_name("output") {
                        out.send(&TextMessage::new(res))?;
                    }
                    Ok(())
                }),
                PortQueueCapacity::Unbounded,
            );

        // Output port: prints received data
        let name_out = name.to_string();
        let output_port: SystemPort<WasmComponent, WasmComponentState, TextMessage> =
            SystemPort::new(
                Box::new(move |_hub, _state, msg| {
                    println!("{} [wasm] OUTPUT: {}", name_out, msg.data);
                    Ok(())
                }),
                PortQueueCapacity::Unbounded,
            );

        let mut ports = HashMap::new();
        ports.insert("input".into(), GenericSystemPort::Text(input_port));
        ports.insert("output".into(), GenericSystemPort::Text(output_port));

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