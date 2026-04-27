use super::node::{NodeOutput, NodeStatus};
use crate::engine::node_registry::NodeRegistry;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ExecutionOptions {
    pub ignore_execute_error: bool,
    pub stdout: bool,
    pub max_async_coroutines: usize,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            ignore_execute_error: false,
            stdout: true,
            max_async_coroutines: 10,
        }
    }
}

/// Shared execution context passed to all nodes
#[derive(Clone)]
pub struct ExecutionContext {
    pub options: ExecutionOptions,
    pub variable: Arc<RwLock<crate::variable::VariableManager>>,
    pub output: Arc<RwLock<OutputCollector>>,
    pub kv: Arc<RwLock<HashMap<String, Value>>>,
    pub run_id: String,
    pub registry: Arc<NodeRegistry>,
    global_index: Arc<std::sync::atomic::AtomicUsize>,
}

impl ExecutionContext {
    pub fn new(
        run_id: String,
        variable: crate::variable::VariableManager,
        options: ExecutionOptions,
        registry: Arc<NodeRegistry>,
    ) -> Self {
        Self {
            options,
            variable: Arc::new(RwLock::new(variable)),
            output: Arc::new(RwLock::new(OutputCollector::new())),
            kv: Arc::new(RwLock::new(HashMap::new())),
            run_id,
            registry,
            global_index: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    pub fn next_global_index(&self) -> usize {
        self.global_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

/// Collects output from all executed nodes
#[derive(Debug, Clone, Default)]
pub struct OutputCollector {
    outputs: Vec<NodeOutput>,
}

impl OutputCollector {
    pub fn new() -> Self {
        Self { outputs: Vec::new() }
    }

    pub fn push(&mut self, output: NodeOutput) {
        self.outputs.push(output);
    }

    pub fn get_all(&self) -> &[NodeOutput] {
        &self.outputs
    }

    pub fn has_errors(&self) -> bool {
        self.outputs.iter().any(|o| o.base.status == NodeStatus::Error)
    }
}
