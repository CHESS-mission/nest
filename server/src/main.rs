use leptos::config::get_configuration;
use server::{parser::parser_specs::TopologySpec, server::NestServer, simulator::Simulator};

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
    TopologySpec::from_yaml("test/test.yaml").unwrap()
}
