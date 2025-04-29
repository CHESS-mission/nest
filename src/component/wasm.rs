use anyhow::Result;
use extism::{Manifest, Plugin, Wasm};
use std::sync::{Arc, Mutex}; 

use super::{
    Component, DynamicSystemPortHub, PortQueueCapacity, SystemPort, SystemPortHub,
    TextMessage, GenericSender
};

pub struct WasmComponent {
    name: String,
    plugin: Arc<Mutex<Plugin>>, // shared plugin instance for multi thread safety !!!
	// mettre dans state a la place 
    system_ports_hub: DynamicSystemPortHub<()>,
}

impl WasmComponent {
    pub fn new(name: &str, wasm_url: &str) -> Result<Self> {
        let url = Wasm::url(wasm_url);
        let manifest = Manifest::new([url]);

        let plugin = Arc::new(Mutex::new(Plugin::new(&manifest, [], true)?));

        let name_string = name.to_string();

        let mut hub = DynamicSystemPortHub::new();

        let plugin_input = Arc::clone(&plugin);
        let name_input = name_string.clone();

        // port that receives input text, runs the WASM plugin, and forwards result to output
        let input_port: SystemPort<DynamicSystemPortHub<()>, (), TextMessage> = SystemPort::new(
            Box::new(move |hub, _state, msg| {
                let mut plugin = plugin_input.lock().unwrap();
                let res = plugin.call::<&str, &str>("count_vowels", &msg.data)?;
                println!("{} [wasm] plugin returned: {}", name_input, res);

                let out_msg = TextMessage::new(&res.to_string()); // wrap plugin output as a TextMessage

                // try sending plugin result to the output port if it's connected
                if let Some(output) = hub.port_by_name("output") {
                    if let GenericSender::Text(sender) = output.get_endpoint() {
                        sender.send(out_msg)?;
                    }
                }

                Ok(())
            }),
            PortQueueCapacity::Unbounded,
        );

        let name_output = name_string.clone();

        // optional output port, just prints the message if received
        let output_port: SystemPort<DynamicSystemPortHub<()>, (), TextMessage> = SystemPort::new(
            Box::new(move |_hub, _state, msg| {
                println!("{} [wasm] output: {}", name_output, msg.data);
                Ok(())
            }),
            PortQueueCapacity::Unbounded,
        );

        hub.add_port("input", Box::new(input_port));
        hub.add_port("output", Box::new(output_port));

        Ok(Self {
            name: name_string,
            plugin,
            system_ports_hub: hub,
        })
    }
}

impl Component<()> for WasmComponent {
    fn system_port_hub(&self) -> &dyn SystemPortHub {
        &self.system_ports_hub
    }

    fn system_port_hub_mut(&mut self) -> &mut dyn SystemPortHub {
        &mut self.system_ports_hub
    }

    fn run(&mut self) -> Result<()> {
        self.system_ports_hub.loop_recv_dispatch(&mut ())
    }
}