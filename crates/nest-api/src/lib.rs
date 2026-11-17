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
    #[serde(flatten)]
    pub value: EnvVarValue,
    #[serde(default)]
    pub lerp_over_s: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum EnvVarValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Vec2(ApiVec2),
    Vec3(ApiVec3),
    Quaternion(ApiQuaternion),
    Tensor(ApiTensor),
}

// Useful for error messages in case of type mismatch
impl EnvVarValue {
    pub fn type_str(&self) -> &'static str {
        match self {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Bool(_) => "bool",
            Self::Vec2(_) => "vec2",
            Self::Vec3(_) => "vec3",
            Self::Quaternion(_) => "quaternion",
            Self::Tensor(_) => "tensor",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiVec2 {
    pub x: f32,
    pub y: f32,
}

impl From<nalgebra::Vector2<f32>> for ApiVec2 {
    fn from(v: nalgebra::Vector2<f32>) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<ApiVec2> for nalgebra::Vector2<f32> {
    fn from(v: ApiVec2) -> Self {
        nalgebra::Vector2::new(v.x, v.y)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<nalgebra::Vector3<f32>> for ApiVec3 {
    fn from(v: nalgebra::Vector3<f32>) -> Self {
        Self { x: v.x, y: v.y, z: v.z }
    }
}

impl From<ApiVec3> for nalgebra::Vector3<f32> {
    fn from(v: ApiVec3) -> Self {
        nalgebra::Vector3::new(v.x, v.y, v.z)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiQuaternion {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<nalgebra::UnitQuaternion<f32>> for ApiQuaternion {
    fn from(q: nalgebra::UnitQuaternion<f32>) -> Self {
        let q = q.quaternion();
        Self {
            w: q.w,
            x: q.i,
            y: q.j,
            z: q.k,
        }
    }
}

impl From<ApiQuaternion> for nalgebra::UnitQuaternion<f32> {
    fn from(q: ApiQuaternion) -> Self {
        nalgebra::UnitQuaternion::from_quaternion(
            nalgebra::Quaternion::new(q.w, q.x, q.y, q.z),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiTensor {
    pub shape: Vec<usize>,

    /// Data is stored in logical order (the rightmost index is varying the fastest).
    pub data: Vec<f32>,
}

impl From<ndarray::ArrayD<f32>> for ApiTensor {
    fn from(arr: ndarray::ArrayD<f32>) -> Self {
        Self {
            shape: arr.shape().to_vec(),
            data: arr.into_iter().collect(),
        }
    }
}

impl TryFrom<ApiTensor> for ndarray::ArrayD<f32> {
    type Error = ndarray::ShapeError;

    fn try_from(t: ApiTensor) -> Result<Self, Self::Error> {
        ndarray::ArrayD::from_shape_vec(t.shape, t.data)
    }
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
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct EventUpdate {
    pub timestamp: f64,
    pub component: String,
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct LogUpdate {
    pub timestamp: f64,
    pub level: LogLevelDTO,
    pub component: String,
    pub message: String,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ErrorUpdate {
    pub timestamp: f64,
    pub component: String,
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
    Mux,
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
