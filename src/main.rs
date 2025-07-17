use nest::{component::{qemu::QemuComponent, test::{TestComponent, TestComponentConfig}, Component, ComponentParams}, ports::{GenericSystemPort, JsonMessage, SystemPort}};
use serde_json::json;

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
    let test_config = TestComponentConfig { initial_sum: 0x0f, greeting: "Hey!".into() };
    let test = TestComponent::new(test_config, SystemPort::default(), SystemPort::default(), test_i2c_port);

    // Start components
    let _test_monitor = test.start().unwrap();
    let _qemu_monitor = qemu.start().unwrap();

    // Infinite loop
    loop {}
}
