use std::time::Instant;
use serde_json::json;

use nest::{
    component::{Component, ComponentParams, LogEntry, qemu::QemuComponent, wasm::WasmComponent},
    ports::{GenericSystemPort, JsonMessage, SystemPort},
};

fn format_log_entry<S: Into<String>>(component_name: S, entry: &LogEntry, start: Instant) -> String {
    let elapsed = entry.timestamp.duration_since(start);

    let total_secs = elapsed.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    let millis = elapsed.subsec_millis();

    let time = format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);

    format!("{}: [{:?}] [{}] {}", time, entry.level, component_name.into(), entry.message)
}

fn main() {
    // Create and connect ports
    let mut qemu_i2c_port: SystemPort<JsonMessage> = SystemPort::default();
    let mut temp_i2c_port: SystemPort<JsonMessage> = SystemPort::default();

    qemu_i2c_port.connect_to(&temp_i2c_port);
    temp_i2c_port.connect_to(&qemu_i2c_port);

    // Instanciate Qemu component
    let mut qemu_params = ComponentParams::new();

    qemu_params.set_config(json!({
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
    }));

    qemu_params.add_system_port("i2c", GenericSystemPort::Json(qemu_i2c_port));

    let qemu = QemuComponent::from_params(&qemu_params).unwrap();

    // Instantiate Temp component
    let mut temp_params = ComponentParams::new();
    temp_params.set_config(json!({
        "module": "component_artifacts/sensor_temp.wasm"
    }));

    temp_params.add_system_port("i2c", GenericSystemPort::Json(temp_i2c_port));

    let temp = WasmComponent::from_params(&temp_params).unwrap();

    // Start components
    let start = Instant::now();
    let temp_monitor = temp.start().unwrap();
    let qemu_monitor = qemu.start().unwrap();

    // Infinite loop
    loop {
        // Print logs received from components
        crossbeam_channel::select! {
            recv(temp_monitor.log_receiver()) -> entry => println!("{}", format_log_entry("TEMP", &entry.unwrap(), start)),
            recv(qemu_monitor.log_receiver()) -> entry => println!("{}", format_log_entry("QEMU", &entry.unwrap(), start)),
        }
    }
}
