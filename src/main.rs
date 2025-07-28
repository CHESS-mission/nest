use std::thread::sleep;
use std::time::{Duration, Instant};

use nest::component::test::TestComponentConfig;
use nest::component::{Component, LogEntry};
use nest::component::{test::TestComponent, ComponentParams};
use nest::ports::{GenericSystemPort, IntMessage, SystemPort, TextMessage};

fn main() {
    // Create component test1 using new flexible parameters approach
    let mut params = ComponentParams::new();

    params.add_config_entry("initial_sum", "42");
    params.add_config_entry("greeting", "Hey!");

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

    // Wait for components to start and handle messages
    sleep(Duration::from_millis(100));

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

    // Stop the components
    monitor1.stop().unwrap();
    monitor2.stop().unwrap();
}
