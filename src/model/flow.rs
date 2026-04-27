use super::node::NodeData;
use super::variable::Variable;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDefinition {
    pub name: Option<String>,
    pub id: Option<String>,
    #[serde(default)]
    pub r#async: bool,
    pub timeout: Option<u64>,
    pub nodes: Vec<NodeData>,
    pub variables: Option<Variable>,
    pub options: Option<u32>,
}
