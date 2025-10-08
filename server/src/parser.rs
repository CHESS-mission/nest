pub mod parser_error;
pub mod parser_specs;

use std::{fs, str::FromStr};

use yaml_rust2::{Yaml, YamlLoader};

use parser_error::ParserError;

use parser_specs::*;

impl TopologySpec {
    pub fn from_yaml(root: &str) -> Result<Self, ParserError> {
        match YamlLoader::load_from_str(match &fs::read_to_string(root) {
            Ok(r) => r,
            Err(_) => return Err(ParserError::DocumentFormat),
        }) {
            Ok(doc) => match doc.last() {
                Some(yaml) => parse_components(yaml),
                None => Err(ParserError::DocumentFormat),
            },
            Err(_) => return Err(ParserError::DocumentFormat),
        }
    }
}

///
/// Builds a Component from a path to a yaml config file.
///
fn parse_components(root: &Yaml) -> Result<TopologySpec, ParserError> {
    let mut environement_variables: Vec<EnvironmentVarSpec> = Vec::new();

    if let Some(env_vars) = root["environment_variables"]
        .as_vec(){
            for env_var in env_vars {
        let name = env_var["name"]
            .as_str()
            .ok_or(ParserError::MissingField("environment_variable.name"))?;

        let port_type = env_var["type"]
            .as_str()
            .ok_or(ParserError::MissingField("environment_variable.port_type"))?
            .parse::<PortTypeSpec>()?;
        environement_variables.push(EnvironmentVarSpec::new(name, port_type))
        }
    }
    

    let comps = root["components"]
        .as_vec()
        .ok_or(ParserError::MissingField("components"))?;

    let mut all_components: Vec<ComponentSpec> = Vec::new();

    for comp in comps {
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
            for entry in sys_ports {
                let port_name = entry["name"]
                    .as_str()
                    .ok_or(ParserError::MissingField("ports.system.name"))?;
                if port_name.trim().is_empty() {
                    return Err(ParserError::ValidationError(
                        "ports.system.name cannot be empty",
                    ));
                }

                let port_type = entry["type"]
                    .as_str()
                    .ok_or(ParserError::MissingField("ports.system.type"))?
                    .parse::<PortTypeSpec>()?;

                system_ports.push(SystemPortSpec::new(port_name, port_type))
            }
        }

        let mut environment_ports: Vec<EnvironmentPortSpec> = Vec::new();
        if let Some(env_ports) = comp["ports"]["environment"].as_vec() {
            for entry in env_ports {
                let port_name = entry["name"]
                    .as_str()
                    .ok_or(ParserError::MissingField("ports.environment.name"))?;

                if port_name.trim().is_empty() {
                    return Err(ParserError::ValidationError(
                        "ports.environment.name cannot be empty",
                    ));
                }

                let port_type = entry["type"]
                    .as_str()
                    .ok_or(ParserError::MissingField("ports.environment.type"))?
                    .parse::<PortTypeSpec>()?;

                environment_ports.push(EnvironmentPortSpec::new(port_name, port_type));
            }
        }

        let component_type = comp["type"]
            .as_str()
            .ok_or(ParserError::ValidationError("components must have a type"))?
            .parse::<ComponentTypeSpec>()?;

        all_components.push(ComponentSpec::new(
            name,
            component_type,
            config,
            system_ports,
            environment_ports,
        ));
    }

    let wires = root["wires"]
        .as_vec()
        .ok_or(ParserError::MissingField("wires"))?;

    let mut connected_wires: Vec<WireSpec> = Vec::new();

    for wire in wires {
        let mut curr_wire: Vec<WireEndpointSpec> = Vec::new();
        let endpoints = wire
            .as_vec()
            .ok_or(ParserError::MissingField("wires.endpoints"))?;

        for endpoint in endpoints {
            let component = endpoint["component"]
                .as_str()
                .ok_or(ParserError::MissingField("wires.component"))?;
            let port = endpoint["port"]
                .as_str()
                .ok_or(ParserError::MissingField("wires.port"))?;
            curr_wire.push(WireEndpointSpec::new(component, port))
        }
        connected_wires.push(WireSpec::new(curr_wire));
    }

    Ok(TopologySpec::new(
        environement_variables,
        all_components,
        connected_wires,
    ))
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
            "json" => PortTypeSpec::Json,
            _ => return Err(ParserError::InvalidType("Unknown port type")),
        })
    }
}

impl FromStr for ComponentTypeSpec {
    type Err = ParserError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().trim() {
            "qemu" => ComponentTypeSpec::Qemu,
            "wasm" => ComponentTypeSpec::Wasm,
            _ => return Err(ParserError::InvalidType("Unknown component type")),
        })
    }
}
