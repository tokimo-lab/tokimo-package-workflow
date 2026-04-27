use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use serde_json::Value;

/// Result of executing a node
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeResult {
    pub id: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub errors: Vec<ErrorOutput>,
    pub timers: NodeTimers,
    pub output: Option<Value>,
    pub extra: Option<Value>,
    pub detail: Option<Value>,
    pub parent_id: Option<String>,
    pub group: Option<usize>,
    pub children: Option<Vec<Vec<NodeResult>>>,
    /// Variables extracted by this node (assignments)
    pub extracted_variables: Variable,
}

impl NodeResult {
    pub fn new(id: String, node_type: NodeType) -> Self {
        Self {
            id,
            node_type,
            status: NodeStatus::Wait,
            errors: Vec::new(),
            timers: NodeTimers::default(),
            output: None,
            extra: None,
            detail: None,
            parent_id: None,
            group: None,
            children: None,
            extracted_variables: Variable::new(),
        }
    }

    pub fn has_error(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn set_error(&mut self, msg: String, code: u32) {
        self.status = NodeStatus::Error;
        self.errors.push(ErrorOutput {
            message: msg,
            stack: None,
            error: code,
            extra: None,
        });
    }

    pub fn total_time(&self) -> f64 {
        let mut total = 0.0;
        if let Some(t) = self.timers.pre_script {
            total += t;
        }
        if let Some(t) = self.timers.execute {
            total += t;
        }
        if let Some(t) = self.timers.post_script {
            total += t;
        }
        if let Some(t) = self.timers.assignment {
            total += t;
        }
        if let Some(t) = self.timers.assert {
            total += t;
        }
        total
    }
}

/// Configuration passed when executing a child node
#[derive(Debug, Clone, Default)]
pub struct NodeExecConfig {
    pub index: usize,
    pub group: Option<usize>,
    pub deep: usize,
    pub parent_id: Option<String>,
    pub skip: bool,
    pub bypass: bool,
    pub local_variables: Variable,
}

/// Trait for atomic nodes (leaf nodes like HTTP, Shell, Script, etc.)
#[async_trait::async_trait]
pub trait AtomicNode: Send + Sync {
    /// Execute the node's main action
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        local_vars: &Variable,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>>;

    /// Node type
    fn node_type(&self) -> NodeType;
}

/// Trait for group nodes (nodes containing child nodes like Loop, Condition, etc.)
#[async_trait::async_trait]
pub trait GroupNode: Send + Sync {
    /// Execute the group node, running child nodes as appropriate
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        children: &[NodeData],
        local_vars: &Variable,
        executor: &crate::engine::executor::FlowExecutor,
    ) -> Result<Vec<Vec<NodeResult>>, Box<dyn std::error::Error + Send + Sync>>;

    fn node_type(&self) -> NodeType;
}
