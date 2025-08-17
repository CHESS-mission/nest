use std::thread::sleep;
use std::time::{Duration, Instant};

use nest::component::test::TestComponentConfig;
use nest::component::{Component, LogEntry};
use nest::component::{test::TestComponent, ComponentParams};
use nest::component::wasm::WasmComponent;
use nest::ports::{GenericSystemPort, IntMessage, SystemPort, TextMessage};
use serde_json::json;

fn main() {
    // Create component test1 using new flexible parameters approach
    let mut params = ComponentParams::new();

    params.set_config(json!({
        "initial_sum": 42,
        "greeting": "Hey!",
    }));

    params.add_system_port("cumsum", GenericSystemPort::Int(SystemPort::default()));
    params.add_system_port("print", GenericSystemPort::Text(SystemPort::default()));

    // Create component test2 using traditional approach
    let mut config = TestComponentConfig::default();
    config.initial_sum = -100;
    config.greeting = "Hello!".into();

    let mut test2_cumsum_port = SystemPort::<IntMessage>::default();
    let mut test2_print_port = SystemPort::<TextMessage>::default();

    // Connect ports
    let test1_cumsum_port = params.system_port_mut("cumsum").unwrap().as_int_mut().unwrap();
    test1_cumsum_port.connect_to(&test2_cumsum_port);
    test2_cumsum_port.connect_to(&test1_cumsum_port);

    let test1_print_port = params.system_port_mut("print").unwrap().as_text_mut().unwrap();
    test1_print_port.connect_to(&test2_print_port);
    test2_print_port.connect_to(&test1_print_port);

    // Manually send messages for testing
    let test1_cumsum_port = params.system_port_mut("cumsum").unwrap().as_int_mut().unwrap();
    test1_cumsum_port.send(&IntMessage::new(10)).unwrap();
    test1_cumsum_port.send(&IntMessage::new(-38)).unwrap();

    test2_cumsum_port.send(&IntMessage::new(8)).unwrap();
    test2_cumsum_port.send(&IntMessage::new(150)).unwrap();
    test2_cumsum_port.send(&IntMessage::new(56)).unwrap();

    // Instantiate both components
    let test1 = TestComponent::from_params(&params).unwrap();
    let test2 = TestComponent::new(config, test2_cumsum_port, test2_print_port);

    // Start component
    let start = Instant::now();
    let monitor1 = test1.start().unwrap();
    let monitor2 = test2.start().unwrap();
    
    // ========== WASM component ==========
    // Prepare params
    let mut wasm_params = ComponentParams::new();
    wasm_params.set_config(json!({
        "module": "component_artifacts/ping_pong_c.wasm",
        "exports": {
            "in": "ping",
        },
        "imports": {
            "out": "pong",
        },
    }));

    wasm_params.add_system_port("in", GenericSystemPort::Text(SystemPort::default()));
    wasm_params.add_system_port("out", GenericSystemPort::Text(SystemPort::default()));

    // Wire external source to the component ports
    let mut wasm_in_src = SystemPort::<TextMessage>::default();
    let wasm_in_port = wasm_params.system_port_mut("in").unwrap().as_text_mut().unwrap();
    wasm_in_src.connect_to(wasm_in_port);

    let wasm_out_sink = SystemPort::<TextMessage>::default();
    let wasm_out_port = wasm_params.system_port_mut("out").unwrap().as_text_mut().unwrap();
    wasm_out_port.connect_to(&wasm_out_sink);

    // Instantiate and start
    let wasm_component = WasmComponent::from_params(&wasm_params).unwrap();
    let monitor_wasm = wasm_component.start().unwrap();

    // Send a sample message to the WASM component
    wasm_in_src.send(&TextMessage::new("hello from host")).unwrap();

    // ========== Temp Sensor WASM component ==========
    let mut temp_params = ComponentParams::new();
    temp_params.set_config(json!({
        "module": "component_artifacts/sensor_temp.wasm",
        "exports": {
            "i2c_in": "i2c_in",
        },
        "imports": {
            "i2c_out": "i2c_out",
        },
    }));

    temp_params.add_system_port("i2c_in", GenericSystemPort::Text(SystemPort::default()));
    temp_params.add_system_port("i2c_out", GenericSystemPort::Text(SystemPort::default()));

    // External source to temp sensor input
    let mut temp_in_src = SystemPort::<TextMessage>::default();
    let temp_in_port = temp_params.system_port_mut("i2c_in").unwrap().as_text_mut().unwrap();
    temp_in_src.connect_to(temp_in_port);

    let temp_sensor_component = WasmComponent::from_params(&temp_params).unwrap();
    let monitor_temp = temp_sensor_component.start().unwrap();

    // Give the temp sensor plugin time to init before sending
    sleep(Duration::from_millis(150));

    // Send messages to the TempSensor plugin
    let msg = serde_json::json!({
        "address": 0x48,
        "data": [0],
        "len": 0,
        "read": false
    }).to_string();
    temp_in_src.send(&TextMessage::new(&msg)).unwrap();

    let msg = serde_json::json!({
        "address": 0x48,
        "data": [],
        "len": 2,
        "read": true
    }).to_string();
    temp_in_src.send(&TextMessage::new(&msg)).unwrap();

    // Wait some more
    sleep(Duration::from_millis(700));

    // Play around reading logs and final status :)
    fn format_log_entry(entry: &LogEntry, start: Instant) -> String {
        let elapsed = entry.timestamp.duration_since(start);

        let total_secs = elapsed.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        let millis = elapsed.subsec_millis();

        let time = format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);

        format!("{}: [{:?}] {}", time, entry.level, entry.message)
    }

    println!("\n====== TestComponent 1 ======");
    println!("\nstatus: {}", serde_json::to_string_pretty(
        &monitor1.status().unwrap()
    ).unwrap());

    println!("\nlogs:");
    while let Ok(entry) = monitor1.log_receiver().try_recv() {
        println!("{}", format_log_entry(&entry, start));
    }

    println!("\n====== TestComponent 2 ======");
    println!("\nstatus: {}", serde_json::to_string_pretty(
        &monitor2.status().unwrap()
    ).unwrap());

    println!("\nlogs:");
    while let Ok(entry) = monitor2.log_receiver().try_recv() {
        println!("{}", format_log_entry(&entry, start));
    }

    println!("\n====== WasmComponent ======");
    println!("\nstatus: {}", serde_json::to_string_pretty(
        &monitor_wasm.status().unwrap()
    ).unwrap());

    println!("\nlogs:");
    while let Ok(entry) = monitor_wasm.log_receiver().try_recv() {
        println!("{}", format_log_entry(&entry, start));
    }

    println!("\n====== TempSensor WasmComponent ======");
    println!("\nstatus: {}", serde_json::to_string_pretty(
        &monitor_temp.status().unwrap()
    ).unwrap());

    println!("\nlogs:");
    while let Ok(entry) = monitor_temp.log_receiver().try_recv() {
        println!("{}", format_log_entry(&entry, start));
    }

    // Stop the components
    monitor1.stop().unwrap();
    monitor2.stop().unwrap();
    monitor_wasm.stop().unwrap();
    monitor_temp.stop().unwrap();
}
