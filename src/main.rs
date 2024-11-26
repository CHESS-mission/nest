use std::collections::{HashMap, VecDeque};
use extism::*;

#[derive(Clone, Debug)]
pub enum PortType {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    Json,
}

#[derive(Clone, Debug)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Clone, Debug)]
pub struct Port {
    pub name: String,
    pub port_type: PortType,
    pub direction: Direction,
}

#[derive(Clone, Debug)]
pub enum Message {
    Int(i32),
    Float(f32),
    Bool(bool),
    Bytes(Vec<u8>),
    Text(String),
    Json(String),
}

pub enum ComponentType {
    Internal, // call it rust ?
    Wasm { plugin: Plugin },
}

pub struct Wire { // use crossbeam for multithread
    pub from: (String, String), // (component_name, port_name)
    pub to: (String, String),
    pub buffer: VecDeque<Message>,
}

impl Wire {
    fn new(from: (String, String), to: (String, String)) -> Self {
        Wire {
            from,
            to,
            buffer: VecDeque::new(),
        }
    }
}

pub struct Component {
    pub name: String,
    pub ports: HashMap<String, Port>, // can have multiple ports, identified by a name
    pub comp_type: ComponentType, // internal or wasm
}

impl Component {
    pub fn handle_message(&mut self, port: &str, msg: &Message) {
        match &mut self.comp_type { // match on component type
            ComponentType::Internal => {
                println!("{} [internal] received on {}: {:?}", self.name, port, msg);
            }
            ComponentType::Wasm { plugin } => {
                if let Message::Text(text) = msg {
                    let res = plugin.call::<&str, &str>("count_vowels", text)
                        .expect("plugin call failed");
                    println!("{} [wasm] plugin returned: {}", self.name, res);
                }
            }
        }
    }
}

// Simulator holds all components and wires
pub struct Simulator {
    pub components: HashMap<String, Component>,
    pub wires: Vec<Wire>,
}

impl Simulator {
    pub fn send_message(&mut self, from: (&str, &str), msg: Message) { // just need "from" which port to send message
        for wire in &mut self.wires { // find corresponding wire and add msg to buffer
            if wire.from == (from.0.to_string(), from.1.to_string()) {
                wire.buffer.push_back(msg.clone());
            }
        }
    }

    pub fn receive_message(&mut self, to: (&str, &str)) -> Option<Message> { // just need "to" which port to receive message
        for wire in &mut self.wires { // find corresponding wire and retrieve msg from buffer
            if wire.to == (to.0.to_string(), to.1.to_string()) {
                return wire.buffer.pop_front();
            }
        }
        None // if no message found
    }
}

fn main() {
    let mut sim = Simulator {
        components: HashMap::new(),
        wires: vec![],
    };

    // Create compA with an output port
    let mut comp_a = Component {
        name: "CompA".to_string(),
        ports: HashMap::new(),
        comp_type: ComponentType::Internal,
    };
    comp_a.ports.insert("out".to_string(), Port {
        name: "out".to_string(),
        port_type: PortType::Text,
        direction: Direction::Output,
    });

    sim.components.insert("CompA".to_string(), comp_a); // add comp to simulation

    // Create compB with an input port (WASM)
    let url = Wasm::url("https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm");
    let manifest = Manifest::new([url]);
    let plugin = Plugin::new(&manifest, [], true).unwrap();

    let mut comp_b = Component {
        name: "CompB".to_string(),
        ports: HashMap::new(),
        comp_type: ComponentType::Wasm { plugin },
    };
    comp_b.ports.insert("in".to_string(), Port {
        name: "in".to_string(),
        port_type: PortType::Text,
        direction: Direction::Input,
    });

    sim.components.insert("CompB".to_string(), comp_b);

    // Wire between CompA out port and CompB in port
    sim.wires.push(Wire::new(
        ("CompA".to_string(), "out".to_string()), // (component_name, port_name)
        ("CompB".to_string(), "in".to_string()),
    ));

    // Send/receive message test
    sim.send_message(("CompA", "out"), Message::Text("Ping!".to_string()));

    if let Some(msg) = sim.receive_message(("CompB", "in")) {
        println!("CompB received: {:?}", msg);
        if let Some(comp) = sim.components.get_mut("CompB") {
            comp.handle_message("in", &msg);
        }
    }
}
