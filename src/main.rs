use std::{thread::sleep, time::Duration};
use nest::component::{
    test::{TestComponent, TestComponentConfig}, wasm::{WasmComponent, WasmComponentConfig}, Component, ConnectablePort, HasPortType, IntMessage, TextMessage,
};

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
    let config_wasm = WasmComponentConfig { name: "WasmComp".into(), wasm_url: "https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm".into() }; 
    let mut wasm_component = WasmComponent::new(config_wasm); 

    // Printer component to display the plugin's output
    let printer_cfg = TestComponentConfig { initial_sum: 0, greeting: "Printer".into() };
    let mut printer = TestComponent::new(printer_cfg);

    // Connect wasm.output --> printer.print
    let wasm_out: &mut dyn ConnectablePort<TextMessage> = wasm_component.port_by_name_mut("output").unwrap();
    let printer_in: &mut dyn ConnectablePort<TextMessage> = printer.port_by_name_mut("print").unwrap();
    wasm_out.connect_to(printer_in);
    
    // Capture sender before starting the component
    let wasm_input = wasm_component.port_by_name_mut("input").unwrap().sender().clone();

    // Start components
    let monitor1 = test1.start().unwrap();
    let monitor2 = test2.start().unwrap();
    let monitor_printer = printer.start().unwrap();
    let monitor_wasm = wasm_component.start().unwrap();

    // Send message to plugin
    wasm_input.send(TextMessage::new("Hello Hola")).unwrap();

    sleep(Duration::from_millis(100));

    monitor1.stop().unwrap();
    monitor2.stop().unwrap();
    monitor_printer.stop().unwrap();
    monitor_wasm.stop().unwrap();
}
