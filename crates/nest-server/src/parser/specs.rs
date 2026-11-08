use nest_api::{
    ComponentDTO, ComponentTypeDTO, EnvPortTypeDTO, EnvironmentVarDTO, PortTypeDTO, SystemPortDTO, TopologyDTO, WireDTO, WireEndpointDTO
};

#[derive(Clone, Debug)]
pub struct TopologySpec {
    pub environment: Vec<EnvironmentVarSpec>,
    pub components: Vec<ComponentSpec>,
    pub wires: Vec<WireSpec>,
}

impl TopologySpec {
    pub fn new(
        environment: Vec<EnvironmentVarSpec>,
        components: Vec<ComponentSpec>,
        wires: Vec<WireSpec>,
    ) -> Self {
        Self {
            environment,
            components,
            wires,
        }
    }
}

impl From<TopologySpec> for TopologyDTO {
    fn from(val: TopologySpec) -> Self {
        TopologyDTO {
            environment: val.environment.into_iter().map(Into::into).collect(),
            components: val.components.into_iter().map(Into::into).collect(),
            wires: val.wires.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnvironmentVarSpec {
    pub name: String,
    pub var_type: EnvPortTypeSpec,
}

impl EnvironmentVarSpec {
    pub fn new<S: Into<String>>(name: S, var_type: EnvPortTypeSpec) -> Self {
        Self { name: name.into(), var_type }
    }
}

impl From<EnvironmentVarSpec> for EnvironmentVarDTO {
    fn from(val: EnvironmentVarSpec) -> Self {
        EnvironmentVarDTO {
            variable: val.name,
            var_type: val.var_type.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ComponentSpec {
    pub name: String,
    pub component_type: ComponentTypeSpec,
    pub config: serde_json::Value,
    pub system_ports: Vec<SystemPortSpec>,
    pub environment_ports: Vec<EnvironmentPortSpec>,
}

impl ComponentSpec {
    pub fn new<S: Into<String>>(
        name: S,
        component_type: ComponentTypeSpec,
        config: serde_json::Value,
        system_ports: Vec<SystemPortSpec>,
        environment_ports: Vec<EnvironmentPortSpec>,
    ) -> Self {
        Self {
            name: name.into(),
            component_type,
            config,
            system_ports,
            environment_ports,
        }
    }
}

impl From<ComponentSpec> for ComponentDTO {
    fn from(val: ComponentSpec) -> Self {
        ComponentDTO {
            name: val.name,
            component_type: val.component_type.into(),
            ports: val.system_ports.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ComponentTypeSpec {
    Qemu,
    Wasm,
    Mux,
}

impl From<ComponentTypeSpec> for ComponentTypeDTO {
    fn from(val: ComponentTypeSpec) -> Self {
        match val {
            ComponentTypeSpec::Qemu => ComponentTypeDTO::Qemu,
            ComponentTypeSpec::Wasm => ComponentTypeDTO::Wasm,
            ComponentTypeSpec::Mux => ComponentTypeDTO::Mux,
        }
    }
}

pub enum PortSpec{
    SystemPortSpec,
    EnvironmentPortSpec,
}

#[derive(Clone, Debug)]
pub struct SystemPortSpec {
    pub name: String,
    pub port_type: PortTypeSpec,
}

impl SystemPortSpec {
    pub fn new<S: Into<String>>(name: S, port_type: PortTypeSpec) -> Self {
        Self {
            name: name.into(),
            port_type,
        }
    }
}

impl From<SystemPortSpec> for SystemPortDTO {
    fn from(val: SystemPortSpec) -> Self {
        SystemPortDTO {
            port: val.name,
            port_type: val.port_type.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnvironmentPortSpec {
    pub name: String,
    pub var_name: String,
    pub port_type: EnvPortTypeSpec,
}

impl EnvironmentPortSpec {
    pub fn new<N: Into<String>, V: Into<String>>(name: N, var_name: V, port_type: EnvPortTypeSpec) -> Self {
        Self { name: name.into(), var_name: var_name.into(), port_type }
    }
}

#[derive(Clone, Debug)]
pub enum PortTypeSpec {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    I2c,
}

impl From<PortTypeSpec> for PortTypeDTO {
    fn from(val: PortTypeSpec) -> Self {
        match val {
            PortTypeSpec::Int => PortTypeDTO::Int,
            PortTypeSpec::Float => PortTypeDTO::Float,
            PortTypeSpec::Bool => PortTypeDTO::Bool,
            PortTypeSpec::Bytes => PortTypeDTO::Bytes,
            PortTypeSpec::Text => PortTypeDTO::Text,
            PortTypeSpec::I2c => PortTypeDTO::I2c,
        }
    }
}

#[derive(Clone, Debug)]
pub enum EnvPortTypeSpec {
    Int,
    Float,
    Bool,
    Vec2,
    Vec3,
    Quaternion,
    Tensor,
}

impl From<EnvPortTypeSpec> for EnvPortTypeDTO {
    fn from(val: EnvPortTypeSpec) -> Self {
        match val {
            EnvPortTypeSpec::Int => EnvPortTypeDTO::Int,
            EnvPortTypeSpec::Float => EnvPortTypeDTO::Float,
            EnvPortTypeSpec::Bool => EnvPortTypeDTO::Bool,
            EnvPortTypeSpec::Vec2 => EnvPortTypeDTO::Vec2,
            EnvPortTypeSpec::Vec3 => EnvPortTypeDTO::Vec3,
            EnvPortTypeSpec::Quaternion => EnvPortTypeDTO::Quaternion,
            EnvPortTypeSpec::Tensor => EnvPortTypeDTO::Tensor,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WireSpec {
    pub endpoints: Vec<WireEndpointSpec>,
}

impl WireSpec {
    pub fn new(endpoints: Vec<WireEndpointSpec>) -> Self {
        Self { endpoints }
    }
}

impl From<WireSpec> for WireDTO {
    fn from(val: WireSpec) -> Self {
        WireDTO {
            endpoints: val.endpoints.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WireEndpointSpec {
    pub component: String,
    pub port: String,
}

impl WireEndpointSpec {
    pub fn new<C: Into<String>, P: Into<String>>(component: C, port: P) -> Self {
        Self {
            component: component.into(),
            port: port.into(),
        }
    }
}

impl From<WireEndpointSpec> for WireEndpointDTO {
    fn from(val: WireEndpointSpec) -> Self {
        WireEndpointDTO {
            component: val.component,
            port: val.port,
        }
    }
}
