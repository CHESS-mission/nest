use clap::Parser;
use std::{net::{SocketAddr, ToSocketAddrs}, path::PathBuf};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Address to bind, e.g. 127.0.0.1:8080 or `[::1]:3000`
    #[arg(short, long,
        env = "NEST_SOCKET_ADDR",
        value_name = "ADDRESS",
        default_value = "localhost:3000",
        value_parser = socket_addr_with_dns
    )]
    pub listen: SocketAddr,

    /// Topology file to load at startup
    #[arg(long, value_name = "FILE")]
    pub topology: Option<PathBuf>,

    /// Start the simulation immediately (requires --topology)
    #[arg(long, requires = "topology")]
    pub start: bool,
}

fn socket_addr_with_dns(s: &str) -> Result<SocketAddr, String> {
    s.to_socket_addrs()
        .map_err(|e| format!("failed to resolve address `{s}`: {e}"))?
        .next()
        .ok_or_else(|| format!("no socket addresses found for `{s}`"))
}

