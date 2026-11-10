pub mod error;
pub mod specs;

use std::{fs, path::PathBuf, str::FromStr};

use yaml_rust2::{Yaml, YamlLoader};

use error::*;

use specs::*;

/// Errors raised when a topology field has an unexpected shape.
fn invalid<S: Into<String>>(msg: S) -> ParserError {
    ParserError::ValidationError(msg.into())
}

impl TopologySpec {
    pub fn from_yaml(yaml: &str) -> Result<Self, ParserError> {
        let docs = match YamlLoader::load_from_str(yaml) {
            Ok(docs) => docs,
            Err(_) => return Err(ParserError::InvalidFormat),
        };

        match docs.as_slice() {
            [doc] => parse_topology(doc),
            // An empty file (or comments only) is an empty document
            [] => parse_topology(&Yaml::Null),
            _ => Err(invalid("a topology must contain a single YAML document")),
        }
    }

    pub fn from_yaml_file(file: &PathBuf) -> Result<Self, ParserError> {
        match fs::read_to_string(file) {
            Ok(yaml) => Self::from_yaml(&yaml),
            Err(e) => Err(ParserError::ReadError(file.clone(), e)),
        }
    }
}

///
/// Parses a Topology from a loaded Topology YAML file.
///
fn parse_topology(root: &Yaml) -> Result<TopologySpec, ParserError> {
    let mut environement_variables: Vec<EnvironmentVarSpec> = Vec::new();
    for env_var in optional_list(root, "environment_variables", "environment_variables")? {
        environement_variables.push(parse_environment_variable(env_var)?)
    }

    let mut all_components: Vec<ComponentSpec> = Vec::new();
    match field(root, "components") {
        None => return Err(ParserError::MissingField("components")),
        Some(Yaml::Array(comps)) => {
            for comp in comps {
                all_components.push(parse_component(comp)?);
            }
        }
        Some(_) => return Err(invalid("components must be a list")),
    }

    let mut connected_wires: Vec<WireSpec> = Vec::new();
    for wire in optional_list(root, "wires", "wires")? {
        connected_wires.push(parse_wire_endpoints(wire)?);
    }

    Ok(TopologySpec::new(
        environement_variables,
        all_components,
        connected_wires,
    ))
}

/// The value of `key` in `node`, if the key is present and non-null.
fn field<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    match &node[key] {
        Yaml::BadValue | Yaml::Null => None,
        other => Some(other),
    }
}

/// An optional list of nodes, validated to be a list when present.
fn optional_list<'a>(node: &'a Yaml, key: &str, path: &str) -> Result<Vec<&'a Yaml>, ParserError> {
    match field(node, key) {
        None => Ok(Vec::new()),
        Some(Yaml::Array(items)) => Ok(items.iter().collect()),
        Some(_) => Err(invalid(format!("{path} must be a list"))),
    }
}

// subfunctions

// env
fn parse_environment_variable(env_var: &Yaml) -> Result<EnvironmentVarSpec, ParserError> {
    let name = required_str(env_var, "name", "environment_variables.name")?;
    let var_type =
        required_str(env_var, "type", "environment_variables.type")?.parse::<EnvPortTypeSpec>()?;

    Ok(EnvironmentVarSpec::new(name, var_type))
}

// components
fn parse_component(comp: &Yaml) -> Result<ComponentSpec, ParserError> {
    let name = required_str(comp, "name", "components.name")?;

    let mut config_map = serde_json::Map::new();
    if let Some(cfg) = field(comp, "config") {
        let Yaml::Hash(cfg) = cfg else {
            return Err(invalid(format!(
                "components.config must be a mapping (got {})",
                type_name(cfg)
            )));
        };
        for (k, v) in cfg {
            let Some(key) = yaml_key_to_string(k) else {
                return Err(invalid(format!(
                    "components.config keys must be scalars (got {})",
                    type_name(k)
                )));
            };
            config_map.insert(key, yaml_to_json(v));
        }
    }
    let config = serde_json::Value::Object(config_map);

    let mut system_ports: Vec<SystemPortSpec> = Vec::new();
    let mut environment_ports: Vec<EnvironmentPortSpec> = Vec::new();

    if let Some(ports) = field(comp, "ports") {
        let Yaml::Hash(groups) = ports else {
            return Err(invalid(format!(
                "components.ports must be a mapping (got {})",
                type_name(ports)
            )));
        };
        for (k, v) in groups {
            let Some(group) = yaml_key_to_string(k) else {
                return Err(invalid(
                    "components.ports groups must be strings".to_string(),
                ));
            };
            match group.as_str() {
                "system" => {
                    for port in typed_list(v, "components.ports.system")? {
                        system_ports.push(parse_system_port(port)?);
                    }
                }
                "environment" => {
                    for port in typed_list(v, "components.ports.environment")? {
                        environment_ports.push(parse_environment_port(port)?);
                    }
                }
                other => {
                    return Err(invalid(format!(
                        "Unknown ports group '{other}': expected 'system' or 'environment'"
                    )));
                }
            }
        }
    }

    let component_type =
        required_str(comp, "type", "components.type")?.parse::<ComponentTypeSpec>()?;

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
    let port_name = required_str(env_port, "name", "ports.environment.name")?;
    let var_name = required_str(env_port, "variable", "ports.environment.variable")?;
    let port_type =
        required_str(env_port, "type", "ports.environment.type")?.parse::<EnvPortTypeSpec>()?;

    Ok(EnvironmentPortSpec::new(port_name, var_name, port_type))
}

// sys port
fn parse_system_port(sys_port: &Yaml) -> Result<SystemPortSpec, ParserError> {
    let port_name = required_str(sys_port, "name", "ports.system.name")?;
    let port_type = required_str(sys_port, "type", "ports.system.type")?.parse::<PortTypeSpec>()?;

    Ok(SystemPortSpec::new(port_name, port_type))
}

// wires
fn parse_wire_endpoints(wire: &Yaml) -> Result<WireSpec, ParserError> {
    let endpoints = typed_list(wire, "wires")?;

    if endpoints.len() < 2 {
        return Err(invalid(
            "every wire must connect at least two endpoints".to_string(),
        ));
    }

    let mut curr_wire: Vec<WireEndpointSpec> = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let component = required_str(endpoint, "component", "wires.component")?;
        let port = required_str(endpoint, "port", "wires.port")?;
        curr_wire.push(WireEndpointSpec::new(component, port))
    }

    Ok(WireSpec::new(curr_wire))
}

// helper functions
/// A required, non-empty string field of a mapping node.
fn required_str<'a>(node: &'a Yaml, key: &str, path: &'static str) -> Result<&'a str, ParserError> {
    match &node[key] {
        Yaml::BadValue | Yaml::Null => Err(ParserError::MissingField(path)),
        Yaml::String(s) if s.trim().is_empty() => Err(invalid(format!("{path} cannot be empty"))),
        Yaml::String(s) => Ok(s.as_str()),
        other => Err(invalid(format!(
            "{path} must be a string (got {})",
            type_name(other)
        ))),
    }
}

/// An optional list of nodes, validated to be a list when given.
fn typed_list<'a>(node: &'a Yaml, path: &str) -> Result<Vec<&'a Yaml>, ParserError> {
    match node {
        Yaml::Array(items) => Ok(items.iter().collect()),
        other => Err(invalid(format!(
            "{path} must be a list (got {})",
            type_name(other)
        ))),
    }
}

/// Human-readable name of a YAML node's shape, for error messages.
fn type_name(node: &Yaml) -> &'static str {
    match node {
        Yaml::Null => "null",
        Yaml::Boolean(_) => "boolean",
        Yaml::Integer(_) => "integer",
        Yaml::Real(_) => "number",
        Yaml::String(_) => "string",
        Yaml::Array(_) => "list",
        Yaml::Hash(_) => "mapping",
        _ => "unknown type",
    }
}

fn yaml_key_to_string(k: &Yaml) -> Option<String> {
    match k {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(n) => Some(n.to_string()),
        Yaml::Real(n) => Some(n.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

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
                if let Some(key) = yaml_key_to_string(k) {
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
            _ => {
                return Err(ParserError::InvalidType(
                    "Unknown environment variable type",
                ))
            }
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
            _ => return Err(ParserError::InvalidType("Unknown component type")),
        })
    }
}
