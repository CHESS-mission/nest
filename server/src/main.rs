use leptos::config::get_configuration;
use serde_json::json;
use server::{
    parser::{
        ComponentSpec, ComponentTypeSpec, EnvironmentVarSpec, PortTypeSpec, SystemPortSpec,
        TopologySpec, WireEndpointSpec, WireSpec,
    },
    server::NestServer,
    simulator::Simulator,
};

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

fn create_topology() -> TopologySpec {
    // Define environment
    let environment: Vec<EnvironmentVarSpec> = Vec::new();

    // Define components
    let mut components: Vec<ComponentSpec> = Vec::new();

    components.push(ComponentSpec::new(
        "QEMU",
        ComponentTypeSpec::Qemu,
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
        vec![SystemPortSpec::new("i2c", PortTypeSpec::Json)],
        vec![],
    ));

    components.push(ComponentSpec::new(
        "TEMP",
        ComponentTypeSpec::Wasm,
        json!({
            "module": "component_artifacts/sensor_temp.wasm"
        }),
        vec![SystemPortSpec::new("i2c", PortTypeSpec::Json)],
        vec![],
    ));

    // Define wires
    let mut wires: Vec<WireSpec> = Vec::new();

    wires.push(WireSpec::new(
        vec![
            WireEndpointSpec::new("QEMU", "i2c"),
            WireEndpointSpec::new("TEMP", "i2c"),
        ]
    ));

    TopologySpec::new(environment, components, wires)
}
