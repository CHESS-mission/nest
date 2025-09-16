// use std::{collections::HashMap, time::Instant};
// use serde_json::json;
//
// use nest::{component::LogEntry, 
//     simulation::{ComponentKind, PortType, RawComponent, SimulationBuilder, Topology, Wire, WireEndpoint}, simulator::Simulator}
// ;
//
// fn format_log_entry(name: &str, entry: &LogEntry, start: Instant) -> String {
//     let elapsed = entry.timestamp.duration_since(start);
//
//     let total_secs = elapsed.as_secs();
//     let hours = total_secs / 3600;
//     let minutes = (total_secs % 3600) / 60;
//     let seconds = total_secs % 60;
//     let millis = elapsed.subsec_millis();
//
//     let time = format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);
//
//     format!("{}: [{:?}] [{}] {}", time, entry.level, name, entry.message)
// }
//
// fn main() {
//     // Define components
//     let mut raw_components: HashMap<String, RawComponent> = HashMap::new();
//
//     raw_components.insert("QEMU".into(), RawComponent::new(
//         ComponentKind::Qemu,
//         json!({
//             "arch": "arm",
//             "mem_mb": 1024,
//             "options": [
//                 { "option": "cpu", "value": "cortex-a7" },
//                 { "option": "M", "value": "virt" },
//                 { "option": "kernel", "value": "./image/zImage" },
//                 { "option": "append", "value": "rootwait root=/dev/vda console=ttyAMA0" },
//                 { "option": "drive", "value": "if=none,file=./image/disk.qcow2,id=hd0" },
//                 { "option": "device", "value": "virtio-blk-device,drive=hd0" },
//                 { "option": "serial", "value": "pty:/tmp/qemu-serial-pty" },
//                 { "option": "nographic" },
//             ],
//             "i2c_timeout_ms": 10,
//             "ngpio": 8,
//             "gpio_names": {
//                 "5": "echo_out",
//                 "7": "echo_in",
//             },
//         }),
//         vec![("i2c".into(), PortType::Json)],
//         vec![],
//     ));
//
//     raw_components.insert("TEMP".into(), RawComponent::new(
//         ComponentKind::Wasm,
//         json!({
//             "module": "component_artifacts/sensor_temp.wasm"
//         }),
//         vec![("i2c".into(), PortType::Json)],
//         vec![],
//     ));
//
//     // Define wires
//     let mut wires: Vec<Wire> = Vec::new();
//
//     wires.push(Wire::new(
//         vec![
//             WireEndpoint::new("QEMU", "i2c"),
//             WireEndpoint::new("TEMP", "i2c"),
//         ]
//     ));
//
//     // Setup & start simulation
//     let topology = Topology::new(raw_components, wires);
//     let mut simulator = Simulator::new();
//     simulator.load_topology(topology).unwrap();
//     simulator.start().unwrap();
//
//     let log_receiver = simulator.log_receiver().unwrap();
//
//     // Infinite loop
//     loop {
//         // Print logs received from components
//         let (name, entry) = &log_receiver.recv();
//         println!("{}", format_log_entry(name, entry, simulator.started_time().unwrap()));
//     }
// }

use std::collections::HashMap;

use api::{Component, ComponentKind, PortType, Topology, Wire, WireEndpoint};
use leptos::config::get_configuration;
use serde_json::json;
use server::{server::NestServer, simulator::Simulator};

#[tokio::main]
async fn main() {
    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;

    let mut simulator = Simulator::new();
    let topology = create_topology();
    simulator.load_topology(topology).unwrap();

    let server = NestServer::new(addr, simulator);
    server.serve().await;
}

fn create_topology() -> Topology {
    // Define components
    let mut raw_components: HashMap<String, Component> = HashMap::new();

    raw_components.insert("QEMU".into(), Component::new(
        ComponentKind::Qemu,
        json!({
            "arch": "arm",
            "mem_mb": 1024,
            "options": [
                { "option": "cpu", "value": "cortex-a7" },
                { "option": "M", "value": "virt" },
                { "option": "kernel", "value": "./image/zImage" },
                { "option": "append", "value": "rootwait root=/dev/vda console=ttyAMA0" },
                { "option": "drive", "value": "if=none,file=./image/disk.qcow2,id=hd0" },
                { "option": "device", "value": "virtio-blk-device,drive=hd0" },
                { "option": "serial", "value": "pty:/tmp/qemu-serial-pty" },
                { "option": "nographic" },
            ],
            "i2c_timeout_ms": 10,
            "ngpio": 8,
            "gpio_names": {
                "5": "echo_out",
                "7": "echo_in",
            },
        }),
        vec![("i2c".into(), PortType::Json)],
        vec![],
    ));

    raw_components.insert("TEMP".into(), Component::new(
        ComponentKind::Wasm,
        json!({
            "module": "component_artifacts/sensor_temp.wasm"
        }),
        vec![("i2c".into(), PortType::Json)],
        vec![],
    ));

    // Define wires
    let mut wires: Vec<Wire> = Vec::new();

    wires.push(Wire::new(
        vec![
            WireEndpoint::new("QEMU", "i2c"),
            WireEndpoint::new("TEMP", "i2c"),
        ]
    ));

    Topology::new(raw_components, wires)
}
