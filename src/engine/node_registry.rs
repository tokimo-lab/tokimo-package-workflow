use crate::model::context::ExecutionContext;
use crate::model::node::{NodeData, NodeType};
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::collections::HashMap;
use std::sync::Arc;

/// Type-erased node executor function
pub type NodeExecutorFn = Arc<
    dyn Fn(
            NodeData,
            ExecutionContext,
            Variable,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct NodeRegistry {
    executors: HashMap<NodeType, NodeExecutorFn>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
        }
    }

    pub fn register(&mut self, node_type: NodeType, executor: NodeExecutorFn) {
        self.executors.insert(node_type, executor);
    }

    pub fn get(&self, node_type: &NodeType) -> Option<&NodeExecutorFn> {
        self.executors.get(node_type)
    }

    pub fn has(&self, node_type: &NodeType) -> bool {
        self.executors.contains_key(node_type)
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
