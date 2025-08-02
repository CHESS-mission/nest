use std::time::{Duration, Instant};
use serde_json::json;

use nest::{
    component::{
        Component, ComponentParams, LogEntry,
        qemu::QemuComponent,
        test::{TestComponent, TestComponentConfig},
    },
    ports::{GenericSystemPort, JsonMessage, SystemPort},
};

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

fn main() {
    // Create and connect ports
    let mut qemu_i2c_port: SystemPort<JsonMessage> = SystemPort::default();
    let mut test_i2c_port: SystemPort<JsonMessage> = SystemPort::default();

    qemu_i2c_port.connect_to(&test_i2c_port);
    test_i2c_port.connect_to(&qemu_i2c_port);

    // Instanciate Qemu component
    let mut qemu_params = ComponentParams::new();

    qemu_params.set_config(json!({
        "arch": "arm",
        "i2c_timeout_ms": 50,
        "options": [
            { "option": "m", "value": "1024" },
            { "option": "cpu", "value": "cortex-a7" },
            { "option": "M", "value": "virt" },
            { "option": "kernel", "value": "./image/zImage" },
            { "option": "append", "value": "rootwait root=/dev/vda console=ttyAMA0" },
            { "option": "drive", "value": "if=none,file=./image/disk.qcow2,id=hd0" },
            { "option": "device", "value": "virtio-blk-device,drive=hd0" },
            { "option": "serial", "value": "mon:stdio" },
            { "option": "nographic" },
        ]
    }));

    qemu_params.add_system_port("i2c", GenericSystemPort::Json(qemu_i2c_port));

    let qemu = QemuComponent::from_params(&qemu_params).unwrap();

    // Instantiate Test component
    let test_config = TestComponentConfig {
        initial_sum: 0x0f,
        greeting: "Hey!".into(),
    };
    let test = TestComponent::new(
        test_config,
        SystemPort::default(),
        SystemPort::default(),
        test_i2c_port,
    );

    // Start components
    let start = Instant::now();
    let _test_monitor = test.start().unwrap();
    let qemu_monitor = qemu.start().unwrap();

    // Infinite loop
    loop {
        // Print logs received from qemu component
        let entry = qemu_monitor.log_receiver().recv().unwrap();
        println!("{}", format_log_entry(&entry, start));
    }
}
