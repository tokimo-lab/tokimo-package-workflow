use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Node Options (bitflags)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeOptions(pub u32);

impl NodeOptions {
    pub const DISABLED: u32 = 1 << 0;
    pub const INTERFACE: u32 = 1 << 1;
    pub const MOCK: u32 = 1 << 2;
    pub const POST_EXECUTE: u32 = 1 << 3;
    pub const BREAKPOINT: u32 = 1 << 4;

    pub fn is_disabled(&self) -> bool {
        self.0 & Self::DISABLED != 0
    }
    pub fn is_post_execute(&self) -> bool {
        self.0 & Self::POST_EXECUTE != 0
    }
}

/// Node execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NodeStatus {
    #[default]
    Wait = 1,
    Running = 2,
    Done = 3,
    Skip = 4,
    Breakpoint = 5,
    Error = 10,
}

/// Execution phase within a node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Init = 1,
    Pre = 2,
    Execute = 3,
    Post = 4,
    Assignment = 5,
    Validator = 6,
    Done = 7,
}

/// All supported node types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum NodeType {
    // Database (30-49)
    Mysql = 30,
    Postgresql = 31,
    Mongodb = 32,
    Redis = 33,
    Mssql = 34,
    // Protocol (50-69)
    Tcp = 51,
    Udp = 53,
    Http = 54,
    Grpc = 55,
    // Flow Control (70-89)
    Loop = 70,
    DataLoop = 71,
    Condition = 72,
    Sleep = 73,
    Poll = 74,
    // Components (90-109)
    Script = 92,
    Assert = 94,
    Assignment = 95,
    // New
    Shell = 200,
}

/// Node error types (bitflags)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct NodeError(pub u32);
impl NodeError {
    pub const PRE_ERROR: u32 = 1 << 1;
    pub const POST_ERROR: u32 = 1 << 2;
    pub const EXECUTE_ERROR: u32 = 1 << 3;
    pub const RESPONSE_ERROR: u32 = 1 << 4;
    pub const ASSIGNMENT_ERROR: u32 = 1 << 5;
    pub const VALIDATOR_ERROR: u32 = 1 << 6;
    pub const SYSTEM_ERROR: u32 = 1 << 7;
    pub const GROUP_ERROR: u32 = 1 << 8;
    pub const UNKNOWN_ERROR: u32 = 1 << 30;
}

/// Timer measurements for each node phase
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeTimers {
    pub pre_script: Option<f64>,
    pub execute: Option<f64>,
    pub post_script: Option<f64>,
    pub assignment: Option<f64>,
    pub assert: Option<f64>,
}

/// Error output from a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorOutput {
    pub message: String,
    pub stack: Option<String>,
    pub error: u32,
    pub extra: Option<Value>,
}

/// Base output from any node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseOutput {
    pub index: usize,
    pub status: NodeStatus,
    pub node_type: NodeType,
    pub errors: Vec<ErrorOutput>,
    pub total_time: f64,
    pub timer: NodeTimers,
}

/// Node output (base + extra detail)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutput {
    #[serde(flatten)]
    pub base: BaseOutput,
    pub detail: Option<Value>,
    pub extra: Option<Value>,
    pub parent_id: Option<String>,
    pub group: Option<usize>,
    /// For group nodes, child outputs
    pub nodes: Option<Vec<Vec<NodeOutput>>>,
}

// === Node-specific data structs ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpNodeData {
    pub method: String,
    pub path: String,
    #[serde(default = "default_https")]
    pub protocol: String,
    pub port: Option<u16>,
    pub hostname: Option<String>,
    pub server_id: Option<String>,
    pub headers: Option<Vec<KeyValue>>,
    pub params: Option<Vec<KeyValue>>,
    pub body: Option<Value>,
    pub form: Option<Vec<KeyValue>>,
    pub config: Option<HttpConfig>,
}

fn default_https() -> String {
    "https".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    pub max_redirects: Option<u32>,
    pub timeout: Option<u64>,
    pub follow_redirect: Option<bool>,
    pub method_rewriting: Option<bool>,
    pub http2: Option<bool>,
    pub retry: Option<u32>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellNodeData {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub working_dir: Option<String>,
    pub env: Option<Vec<KeyValue>>,
    pub timeout: Option<u64>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpNodeData {
    pub host: String,
    pub port: u16,
    pub data: Option<String>,
    pub timeout: Option<u64>,
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpNodeData {
    pub host: String,
    pub port: u16,
    pub data: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcNodeData {
    pub host: String,
    pub port: u16,
    pub service: String,
    pub method: String,
    pub body: Option<Value>,
    pub metadata: Option<Vec<KeyValue>>,
    pub proto_file: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionNodeData {
    #[serde(default)]
    pub mode: u8,
    pub condition: String,
    pub nodes: Vec<Vec<NodeData>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopNodeData {
    pub count: Value,
    pub data: Option<Vec<Value>>,
    pub config: Option<LoopConfig>,
    pub nodes: Vec<NodeData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    #[serde(default = "default_index_var")]
    pub index_var: String,
    #[serde(default)]
    pub r#async: bool,
    #[serde(default)]
    pub ignore_error: bool,
}

fn default_index_var() -> String {
    "index".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLoopNodeData {
    pub data: Vec<Value>,
    pub config: Option<LoopConfig>,
    pub nodes: Vec<NodeData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepNodeData {
    pub time: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollNodeData {
    pub interval: u64,
    pub max_attempts: u32,
    pub condition: String,
    pub nodes: Vec<NodeData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptNodeData {
    pub script: String,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertNodeData {
    pub asserts: Vec<AssertItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertItem {
    pub source: String,
    pub method: String,
    pub expect: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentNodeData {
    pub assignments: Vec<AssignmentItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentItem {
    pub variable: String,
    pub method: String,
    pub expression: String,
}

// Database node data structs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MysqlNodeData {
    pub command: String,
    pub server_id: Option<String>,
    pub connection: Option<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresqlNodeData {
    pub command: String,
    pub server_id: Option<String>,
    pub connection: Option<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MongodbNodeData {
    pub command: String,
    pub collection: Option<String>,
    pub server_id: Option<String>,
    pub connection: Option<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisNodeData {
    pub command: String,
    pub server_id: Option<String>,
    pub connection: Option<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MssqlNodeData {
    pub command: String,
    pub server_id: Option<String>,
    pub connection: Option<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConnection {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
}

/// The unified `NodeData` struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    /// Node type
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Optional node ID
    pub id: Option<String>,
    /// Node display name
    pub name: Option<String>,
    /// Remark/comment
    pub remark: Option<String>,
    /// Options bitflags
    #[serde(default)]
    pub options: NodeOptions,
    /// Pre-execution scripts
    pub scripts: Option<Vec<ProcessScript>>,
    /// Assertions
    pub asserts: Option<Vec<AssertItem>>,
    /// Assignments
    pub assignments: Option<Vec<AssignmentItem>>,
    /// Type-specific data
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessScript {
    #[serde(rename = "type")]
    pub script_type: u8,
    pub script: String,
}
