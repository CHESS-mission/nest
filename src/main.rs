use std::{thread::sleep, time::Duration};
use nest::component::{
    test::{TestComponent, TestComponentConfig}, wasm::{WasmComponent, WasmComponentConfig}, Component, ConnectablePort, HasPortType, IntMessage, TextMessage,
};
use nest::component::wasm::{InputPort, OutputPort, MessageType::Text};
use serde_json;



fn main() {
    // Instantiate components
    let config1 = TestComponentConfig { initial_sum: 42, greeting: "Hey!".into() };
    let mut test1 = TestComponent::new(config1);

    let config2 = TestComponentConfig { initial_sum: -100, greeting: "Hello!".into() };
    let mut test2 = TestComponent::new(config2);

    // Connect ports
    let print1: &mut dyn ConnectablePort<TextMessage> = test1.port_by_name_mut("print").unwrap();
    let print2: &mut dyn ConnectablePort<TextMessage> = test2.port_by_name_mut("print").unwrap();

    print1.connect_to(print2);
    print2.connect_to(print1);

    let cumsum1 = test1.port_by_name_mut("cumsum").unwrap();
    let cumsum2 = test2.port_by_name_mut("cumsum").unwrap();

    cumsum1.connect_to(cumsum2);
    cumsum2.connect_to(cumsum1);

    // Manually send messages
    cumsum1.send(&IntMessage::new(10)).unwrap();
    cumsum1.send(&IntMessage::new(-38)).unwrap();

    cumsum2.send(&IntMessage::new(8)).unwrap();
    cumsum2.send(&IntMessage::new(150)).unwrap();
    cumsum2.send(&IntMessage::new(56)).unwrap();

    // WasmComponent test
    let config_wasm = WasmComponentConfig {
        name: "WasmComp".into(),
        wasm_url: "https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm".into(),
        input_ports: vec![ InputPort { name: "count_vowels".into(), msg_type: Text } ],
        output_ports: vec![],
    };    
    let mut wasm_component = WasmComponent::new(config_wasm); 

    // Ping Pong WasmComponent
    let config_pingpong = WasmComponentConfig {
        name: "PingPong".into(),
        wasm_url: "component_artifacts/ping_pong_c.wasm".into(),
        input_ports: vec![ InputPort { name: "ping".into(), msg_type: Text } ],
        output_ports: vec![ OutputPort { name: "pong".into(), msg_type: Text } ],
    };    
    let mut pingpong_component = WasmComponent::new(config_pingpong);

    // Printer component to display the plugin's output
    let printer_cfg = TestComponentConfig { initial_sum: 0, greeting: "Printer".into() };
    let mut printer = TestComponent::new(printer_cfg);

    let printer_in: &mut dyn ConnectablePort<TextMessage> = printer.port_by_name_mut("print").unwrap();
    
    // Connect pingpong.output --> printer.print
    let pong_out: &mut dyn ConnectablePort<TextMessage> =
    pingpong_component.port_by_name_mut("pong").unwrap();
    pong_out.connect_to(printer_in);

    // Capture sender before starting the component
    let wasm_input = wasm_component.port_by_name("count_vowels").unwrap().sender().clone();
    let pingpong_input = pingpong_component.port_by_name("ping").unwrap().sender().clone();


    // TempSensor component
    let config_temp_sensor = WasmComponentConfig {
        name: "TempSensor".into(),
        wasm_url: "component_artifacts/sensor_temp.wasm".into(),
        input_ports: vec![ InputPort { name: "i2c_in".into(), msg_type: Text } ],
        output_ports: vec![ OutputPort { name: "i2c_out".into(), msg_type: Text } ],
    };
    let mut temp_sensor = WasmComponent::new(config_temp_sensor);

    // Printer for TempSensor
    let printer_cfg = TestComponentConfig { initial_sum: 0, greeting: "TempPrinter".into() };
    let mut temp_printer = TestComponent::new(printer_cfg);

    // Connect TempSensor output to printer input
    let printer_in: &mut dyn ConnectablePort<TextMessage> = temp_printer.port_by_name_mut("print").unwrap();
    let i2c_out_port: &mut dyn ConnectablePort<TextMessage> = temp_sensor.port_by_name_mut("i2c_out").unwrap();
    i2c_out_port.connect_to(printer_in);

    // Capture sender before starting the component
    let temp_input = temp_sensor.port_by_name("i2c_in").unwrap().sender().clone();

    // Start components
    let monitor1 = test1.start().unwrap();
    let monitor2 = test2.start().unwrap();
    let monitor_printer = printer.start().unwrap();
    let monitor_wasm = wasm_component.start().unwrap();
    let monitor_pingpong = pingpong_component.start().unwrap();
    let monitor_temp_sensor = temp_sensor.start().unwrap();
    let monitor_temp_printer = temp_printer.start().unwrap();

    // Send message to PingPong plugin
    wasm_input.send(TextMessage::new("Hello Hola")).unwrap();
    pingpong_input.send(TextMessage::new("ping")).unwrap();

    // Send messages to the TempSensor plugin
    let msg = serde_json::json!({
        "address": 0x48,
        "data": [0],
        "len": 0,
        "read": false
    }).to_string();
    temp_input.send(TextMessage::new(&msg)).unwrap();

    let msg = serde_json::json!({
        "address": 0x48,
        "data": [],
        "len": 2,
        "read": true
    }).to_string();
    temp_input.send(TextMessage::new(&msg)).unwrap();

    sleep(Duration::from_millis(300));

    monitor1.stop().unwrap();
    monitor2.stop().unwrap();
    monitor_printer.stop().unwrap();
    monitor_wasm.stop().unwrap();
    monitor_pingpong.stop().unwrap();
    monitor_temp_sensor.stop().unwrap();
    monitor_temp_printer.stop().unwrap();
}
