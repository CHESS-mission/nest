pub mod error;
pub mod specs;

use std::{fs, path::PathBuf, str::FromStr};

use yaml_rust2::{Yaml, YamlLoader};

use error::*;

use specs::*;

impl TopologySpec {
    pub fn from_yaml(yaml: &str) -> Result<Self, ParserError> {
        match YamlLoader::load_from_str(yaml) {
            Ok(doc) => match doc.last() {
                Some(yaml) => parse_topology(yaml),
                None => Err(ParserError::InvalidFormat),
            },
            Err(_) => return Err(ParserError::InvalidFormat),
        }
    }

    pub fn from_yaml_file(file: &PathBuf) -> Result<Self, ParserError> {
        Self::from_yaml(match &fs::read_to_string(file) {
            Ok(r) => r,
            Err(_) => return Err(ParserError::InvalidFormat),
        }) 
    }
}

///
/// Parses a Topology from a loaded Topology YAML file.
///
fn parse_topology(root: &Yaml) -> Result<TopologySpec, ParserError> {
    let mut environement_variables: Vec<EnvironmentVarSpec> = Vec::new();
    if let Some(env_vars) = root["environment_variables"].as_vec() {
        for env_var in env_vars {
            environement_variables.push(parse_environment_variable(env_var)?)
        }
    }

    let comps = root["components"]
        .as_vec()
        .ok_or(ParserError::MissingField("components"))?;

    let mut all_components: Vec<ComponentSpec> = Vec::new();
    for comp in comps {
        all_components.push(parse_component(comp)?);
    }

    let mut connected_wires: Vec<WireSpec> = Vec::new();
    if let Some(wires) = root["wires"].as_vec() {
        for wire in wires {
            connected_wires.push(parse_wire_endpoints(wire)?);
        }
    }

    Ok(TopologySpec::new(
        environement_variables,
        all_components,
        connected_wires,
    ))
}

// subfunctions

// env
fn parse_environment_variable(env_var: &Yaml) -> Result<EnvironmentVarSpec, ParserError> {
    let name = env_var["name"]
        .as_str()
        .ok_or(ParserError::MissingField("environment_variable.name"))?;

    let var_type = env_var["type"]
        .as_str()
        .ok_or(ParserError::MissingField("environment_variable.type"))?
        .parse::<EnvPortTypeSpec>()?;

    Ok(EnvironmentVarSpec::new(name, var_type))
}

// components
fn parse_component(comp: &Yaml) -> Result<ComponentSpec, ParserError> {
    let name = comp["name"]
        .as_str()
        .ok_or(ParserError::MissingField("name"))?;

    let mut config_map = serde_json::Map::new();
    if let Some(cfg) = comp["config"].as_hash() {
        for (k, v) in cfg {
            if let Some(key) = k.as_str() {
                config_map.insert(key.to_string(), yaml_to_json(v));
            }
        }
    }
    let config = serde_json::Value::Object(config_map);

    let mut system_ports: Vec<SystemPortSpec> = Vec::new();

    if let Some(sys_ports) = comp["ports"]["system"].as_vec() {
        for sys_port in sys_ports {
            system_ports.push(parse_system_port(sys_port)?)
        }
    }

    let mut environment_ports: Vec<EnvironmentPortSpec> = Vec::new();
    if let Some(env_ports) = comp["ports"]["environment"].as_vec() {
        for env_port in env_ports {
            environment_ports.push(parse_environment_port(env_port)?);
        }
    }

    let component_type = comp["type"]
        .as_str()
        .ok_or(ParserError::ValidationError("components must have a type"))?
        .parse::<ComponentTypeSpec>()?;

    Ok(ComponentSpec::new(
        name,
        component_type,
        config,
        system_ports,
        environment_ports,
    ))
}

// Although these two might seem similar, we will keep them seperate for the time being 
// as they do very different things.

// env port
fn parse_environment_port(env_port: &Yaml) -> Result<EnvironmentPortSpec, ParserError> {
    let port_name = env_port["name"]
        .as_str()
        .ok_or(ParserError::MissingField("ports.environment.name"))?;

    if port_name.trim().is_empty() {
        return Err(ParserError::ValidationError(
            "ports.environment.name cannot be empty",
        ));
    }

    let var_name = env_port["variable"]
        .as_str()
        .ok_or(ParserError::MissingField("ports.environment.variable"))?;

    let port_type = env_port["type"]
        .as_str()
        .ok_or(ParserError::MissingField("ports.environment.type"))?
        .parse::<EnvPortTypeSpec>()?;

    Ok(EnvironmentPortSpec::new(port_name, var_name, port_type))
}

// sys port
fn parse_system_port(sys_port: &Yaml) -> Result<SystemPortSpec, ParserError> {
    let port_name = sys_port["name"]
        .as_str()
        .ok_or(ParserError::MissingField("ports.system.name"))?;

    if port_name.trim().is_empty() {
        return Err(ParserError::ValidationError(
            "ports.system.name cannot be empty",
        ));
    }

    let port_type = sys_port["type"]
        .as_str()
        .ok_or(ParserError::MissingField("ports.system.type"))?
        .parse::<PortTypeSpec>()?;

    Ok(SystemPortSpec::new(port_name, port_type))
}


// wires
fn parse_wire_endpoints(wire: &Yaml) -> Result<WireSpec, ParserError> {
    let mut curr_wire: Vec<WireEndpointSpec> = Vec::new();

    if let Some(endpoints) = wire.as_vec() {
        for endpoint in endpoints {
            let component = endpoint["component"]
                .as_str()
                .ok_or(ParserError::MissingField("wires.component"))?;
            let port = endpoint["port"]
                .as_str()
                .ok_or(ParserError::MissingField("wires.port"))?;
            curr_wire.push(WireEndpointSpec::new(component, port))
        }
    }

    Ok(WireSpec::new(curr_wire))
}

// helper functions
fn yaml_to_json(y: &Yaml) -> serde_json::Value {
    match y {
        Yaml::Real(s) => {
            if let Ok(n) = s.parse::<f64>() {
                serde_json::Value::from(n)
            } else {
                serde_json::Value::String(s.clone())
            }
        }
        Yaml::Integer(i) => serde_json::Value::from(*i),
        Yaml::Boolean(b) => serde_json::Value::from(*b),
        Yaml::String(s) => serde_json::Value::String(s.clone()),
        Yaml::Array(arr) => serde_json::Value::Array(arr.iter().map(yaml_to_json).collect()),
        Yaml::Hash(hm) => {
            let mut map = serde_json::Map::new();
            for (k, v) in hm {
                if let Some(key) = k.as_str() {
                    map.insert(key.to_string(), yaml_to_json(v));
                }
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

impl FromStr for PortTypeSpec {
    type Err = ParserError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().trim() {
            "int" => PortTypeSpec::Int,
            "float" => PortTypeSpec::Float,
            "bool" => PortTypeSpec::Bool,
            "bytes" => PortTypeSpec::Bytes,
            "text" => PortTypeSpec::Text,
            "i2c" => PortTypeSpec::I2c,
            _ => return Err(ParserError::InvalidType("Unknown port type")),
        })
    }
}

impl FromStr for EnvPortTypeSpec {
    type Err = ParserError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().trim() {
            "int" => EnvPortTypeSpec::Int,
            "float" => EnvPortTypeSpec::Float,
            "bool" => EnvPortTypeSpec::Bool,
            "vec2" => EnvPortTypeSpec::Vec2,
            "vec3" => EnvPortTypeSpec::Vec3,
            "quaternion" => EnvPortTypeSpec::Quaternion,
            "tensor" => EnvPortTypeSpec::Tensor,
            _ => return Err(ParserError::InvalidType("Unknown environment variable type")),
        })
    }
}

impl FromStr for ComponentTypeSpec {
    type Err = ParserError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().trim() {
            "qemu" => ComponentTypeSpec::Qemu,
            "wasm" => ComponentTypeSpec::Wasm,
            "mux" => ComponentTypeSpec::Mux,
            "fuzzer" => ComponentTypeSpec::Fuzzer,
            _ => return Err(ParserError::InvalidType("Unknown component type")),
        })
    }
}

