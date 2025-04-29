// use std::collections::{HashMap, VecDeque};
// use extism::*;
//
// #[derive(Clone, Debug)]
// pub enum PortType {
//     Int,
//     Float,
//     Bool,
//     Bytes,
//     Text,
//     Json,
// }
//
// #[derive(Clone, Debug)]
// pub enum Direction {
//     Input,
//     Output,
// }
//
// #[derive(Clone, Debug)]
// pub struct Port {
//     pub name: String,
//     pub port_type: PortType,
//     pub direction: Direction,
// }
//
// #[derive(Clone, Debug)]
// pub enum Message {
//     Int(i32),
//     Float(f32),
//     Bool(bool),
//     Bytes(Vec<u8>),
//     Text(String),
//     Json(String),
// }
//
// pub enum ComponentType {
//     Internal, // call it rust ?
//     Wasm { plugin: Plugin },
// }
//
// pub struct Wire { // use crossbeam for multithread
//     pub from: (String, String), // (component_name, port_name)
//     pub to: (String, String),
//     pub buffer: VecDeque<Message>,
// }
//
// impl Wire {
//     fn new(from: (String, String), to: (String, String)) -> Self {
//         Wire {
//             from,
//             to,
//             buffer: VecDeque::new(),
//         }
//     }
// }
//
// pub struct Component {
//     pub name: String,
//     pub ports: HashMap<String, Port>, // can have multiple ports, identified by a name
//     pub comp_type: ComponentType, // internal or wasm
// }
//
// impl Component {
//     pub fn handle_message(&mut self, port: &str, msg: &Message) {
//         match &mut self.comp_type { // match on component type
//             ComponentType::Internal => {
//                 println!("{} [internal] received on {}: {:?}", self.name, port, msg);
//             }
//             ComponentType::Wasm { plugin } => {
//                 if let Message::Text(text) = msg {
//                     let res = plugin.call::<&str, &str>("count_vowels", text)
//                         .expect("plugin call failed");
//                     println!("{} [wasm] plugin returned: {}", self.name, res);
//                 }
//             }
//         }
//     }
// }
//
// // Simulator holds all components and wires
// pub struct Simulator {
//     pub components: HashMap<String, Component>,
//     pub wires: Vec<Wire>,
// }
//
// impl Simulator {
//     pub fn send_message(&mut self, from: (&str, &str), msg: Message) { // just need "from" which port to send message
//         for wire in &mut self.wires { // find corresponding wire and add msg to buffer
//             if wire.from == (from.0.to_string(), from.1.to_string()) {
//                 wire.buffer.push_back(msg.clone());
//             }
//         }
//     }
//
//     pub fn receive_message(&mut self, to: (&str, &str)) -> Option<Message> { // just need "to" which port to receive message
//         for wire in &mut self.wires { // find corresponding wire and retrieve msg from buffer
//             if wire.to == (to.0.to_string(), to.1.to_string()) {
//                 return wire.buffer.pop_front();
//             }
//         }
//         None // if no message found
//     }
// }
//
// fn main() {
//     let mut sim = Simulator {
//         components: HashMap::new(),
//         wires: vec![],
//     };
//
//     // Create compA with an output port
//     let mut comp_a = Component {
//         name: "CompA".to_string(),
//         ports: HashMap::new(),
//         comp_type: ComponentType::Internal,
//     };
//     comp_a.ports.insert("out".to_string(), Port {
//         name: "out".to_string(),
//         port_type: PortType::Text,
//         direction: Direction::Output,
//     });
//
//     sim.components.insert("CompA".to_string(), comp_a); // add comp to simulation
//
//     // Create compB with an input port (WASM)
//     let url = Wasm::url("https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm");
//     let manifest = Manifest::new([url]);
//     let plugin = Plugin::new(&manifest, [], true).unwrap();
//
//     let mut comp_b = Component {
//         name: "CompB".to_string(),
//         ports: HashMap::new(),
//         comp_type: ComponentType::Wasm { plugin },
//     };
//     comp_b.ports.insert("in".to_string(), Port {
//         name: "in".to_string(),
//         port_type: PortType::Text,
//         direction: Direction::Input,
//     });
//
//     sim.components.insert("CompB".to_string(), comp_b);
//
//     // Wire between CompA out port and CompB in port
//     sim.wires.push(Wire::new(
//         ("CompA".to_string(), "out".to_string()), // (component_name, port_name)
//         ("CompB".to_string(), "in".to_string()),
//     ));
//
//     // Send/receive message test
//     sim.send_message(("CompA", "out"), Message::Text("Ping!".to_string()));
//
//     if let Some(msg) = sim.receive_message(("CompB", "in")) {
//         println!("CompB received: {:?}", msg);
//         if let Some(comp) = sim.components.get_mut("CompB") {
//             comp.handle_message("in", &msg);
//         }
//     }
// }

use std::thread;

use nest::component::{test::{TestComponent, TestComponentConfig}, Component, GenericSender, IntMessage, wasm::WasmComponent, TextMessage};

fn main() {
    // Instantiate components
    let config1 = TestComponentConfig { initial_sum: 42, greeting: "Hey!".into() };
    let mut test1 = TestComponent::new(config1);

    let config2 = TestComponentConfig { initial_sum: -100, greeting: "Hello!".into() };
    let mut test2 = TestComponent::new(config2);

    // Instantiate WasmComponent
    let wasm_url = "https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm";
    let mut wasm_component = WasmComponent::new("WasmComp", wasm_url).unwrap();


    // Connect ports
    let print1 = test1.system_port_hub_mut().port_by_name_mut("print").unwrap();
    let print2 = test2.system_port_hub_mut().port_by_name_mut("print").unwrap();

    print1.connect_endpoint(&print2.get_endpoint()).unwrap();
    print2.connect_endpoint(&print1.get_endpoint()).unwrap();


    // I can't just do that because of borrowing, how should I deal with that when connecting input and output ports? !!!
    let wasm_input = wasm_component.system_port_hub_mut().port_by_name_mut("input").unwrap().get_endpoint();
    let wasm_output = wasm_component.system_port_hub_mut().port_by_name_mut("output").unwrap().get_endpoint();

    // Have to split scope of borrowings
    // let wasm_input = {
    //     let hub = wasm_component.system_port_hub_mut();
    //     hub.port_by_name_mut("input").unwrap().get_endpoint()
    // };    
    // {
    //     let hub = wasm_component.system_port_hub_mut();
    //     let output = hub.port_by_name_mut("output").unwrap();
    //     output.connect_endpoint(&print1.get_endpoint()).unwrap(); // connect output port of wasm_component to print port of test1
    // }

    // Manually send messages
    let cumsum1 = test1.system_port_hub().port_by_name("cumsum").unwrap();
    let cumsum2 = test2.system_port_hub().port_by_name("cumsum").unwrap();

    if let GenericSender::Int(tx) = cumsum1.get_endpoint() {
        tx.send(IntMessage::new(8)).unwrap();
        tx.send(IntMessage::new(150)).unwrap();
        tx.send(IntMessage::new(56)).unwrap();
    }

    if let GenericSender::Int(tx) = cumsum2.get_endpoint() {
        tx.send(IntMessage::new(10)).unwrap();
        tx.send(IntMessage::new(-38)).unwrap();
    }

    if let GenericSender::Text(tx) = wasm_input { // already an endpoint
        tx.send(TextMessage::new("Hello Hola")).unwrap();
    }

    // Start components
    let handle1 = thread::spawn(move || {
        test1.run().unwrap();
    });

    let handle2 = thread::spawn(move || {
        test2.run().unwrap();
    });

    let wasm_handle = thread::spawn(move || {
        wasm_component.run().unwrap();
    });
    

    handle1.join().unwrap();
    handle2.join().unwrap();
    wasm_handle.join().unwrap()
}
