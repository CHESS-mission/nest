use time::OffsetDateTime;

use serde::{Deserialize, Serialize};

// Actions (client -> server)

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "action")]
#[serde(rename_all = "snake_case")]
pub enum NestApiAction {
    Start,
    Stop,
    UploadTopology(UploadTopologyAction),
    Command(CommandAction),
    SetEnv(SetEnvAction),
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct UploadTopologyAction {
    pub topology_yaml: String,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct CommandAction {
    pub component: String,
    pub command: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct SetEnvAction {
    pub variable: String,
    pub value: serde_json::Value,
}

// Updates (server -> client)

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NestApiUpdate {
    Status(StatusUpdate),
    Event(EventUpdate),
    Log(LogUpdate),
    Error(ErrorUpdate),
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "state")]
#[serde(rename_all = "snake_case")]
pub enum StatusUpdate {
    Idle,
    Ready(ReadyStatusUpdate),
    Running(RunningStatusUpdate),
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ReadyStatusUpdate {
    pub topology: TopologyDTO,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct RunningStatusUpdate {
    pub topology: TopologyDTO,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct EventUpdate {
    pub component: String,
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct LogUpdate {
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub level: LogLevelDTO,
    pub component: String,
    pub message: String,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ErrorUpdate {
    pub message: String,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevelDTO {
    Info,
    Warning,
    Error,
}

// Topology DTO

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct TopologyDTO {
    pub environment: Vec<EnvironmentVarDTO>,
    pub components: Vec<ComponentDTO>,
    pub wires: Vec<WireDTO>,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct EnvironmentVarDTO {
    pub variable: String,
    #[serde(rename = "type")]
    pub var_type: EnvPortTypeDTO,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ComponentDTO {
    pub name: String,
    #[serde(rename = "type")]
    pub component_type: ComponentTypeDTO,
    pub ports: Vec<SystemPortDTO>,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentTypeDTO {
    Qemu,
    Wasm,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct SystemPortDTO {
    pub port: String,
    #[serde(rename = "type")]
    pub port_type: PortTypeDTO,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortTypeDTO {
    Int,
    Float,
    Bool,
    Bytes,
    Text,
    I2c,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvPortTypeDTO {
    Int,
    Float,
    Bool,
    Vec2,
    Vec3,
    Quaternion,
    Tensor,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct WireDTO {
    pub endpoints: Vec<WireEndpointDTO>,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct WireEndpointDTO {
    pub component: String,
    pub port: String,
}
