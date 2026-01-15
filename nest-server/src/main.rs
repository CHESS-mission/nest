use tracing_subscriber::EnvFilter;
use nest_server::{
    parser::specs::TopologySpec, server::NestServer, simulator::Simulator
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let conf = leptos::config::get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;

    let mut simulator = Simulator::new();
    let topology = create_topology();
    simulator.load_topology(topology).unwrap();

    let server = NestServer::new(addr, simulator);
    server.serve().await;
}

fn create_topology() -> TopologySpec {
    TopologySpec::from_yaml("example/example.yaml").unwrap()
}
