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

impl Into<TopologyDTO> for TopologySpec {
    fn into(self) -> TopologyDTO {
        TopologyDTO {
            environment: self.environment.into_iter().map(Into::into).collect(),
            components: self.components.into_iter().map(Into::into).collect(),
            wires: self.wires.into_iter().map(Into::into).collect(),
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

impl Into<EnvironmentVarDTO> for EnvironmentVarSpec {
    fn into(self) -> EnvironmentVarDTO {
        EnvironmentVarDTO {
            variable: self.name,
            var_type: self.var_type.into(),
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

impl Into<ComponentDTO> for ComponentSpec {
    fn into(self) -> ComponentDTO {
        ComponentDTO {
            name: self.name,
            component_type: self.component_type.into(),
            ports: self.system_ports.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ComponentTypeSpec {
    Qemu,
    Wasm,
}

impl Into<ComponentTypeDTO> for ComponentTypeSpec {
    fn into(self) -> ComponentTypeDTO {
        match self {
            ComponentTypeSpec::Qemu => ComponentTypeDTO::Qemu,
            ComponentTypeSpec::Wasm => ComponentTypeDTO::Wasm,
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

impl Into<SystemPortDTO> for SystemPortSpec {
    fn into(self) -> SystemPortDTO {
        SystemPortDTO {
            port: self.name,
            port_type: self.port_type.into(),
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

impl Into<PortTypeDTO> for PortTypeSpec {
    fn into(self) -> PortTypeDTO {
        match self {
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

impl Into<EnvPortTypeDTO> for EnvPortTypeSpec {
    fn into(self) -> EnvPortTypeDTO {
        match self {
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

impl Into<WireDTO> for WireSpec {
    fn into(self) -> WireDTO {
        WireDTO {
            endpoints: self.endpoints.into_iter().map(Into::into).collect(),
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

impl Into<WireEndpointDTO> for WireEndpointSpec {
    fn into(self) -> WireEndpointDTO {
        WireEndpointDTO {
            component: self.component,
            port: self.port,
        }
    }
}
