pub mod engine;
pub mod model;
pub mod node;
pub mod trigger;
pub mod variable;

use engine::executor::FlowExecutor;
use engine::node_registry::NodeRegistry;
use engine::task_manager::{TaskCompletion, TaskId, TaskManager, TaskProgress};
use model::flow::FlowDefinition;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Main entry point for the Think Flow engine
pub struct ThinkFlow {
    task_manager: TaskManager,
    registry: Arc<NodeRegistry>,
}

impl ThinkFlow {
    /// Create a new `ThinkFlow` engine with all built-in nodes registered
    pub fn new() -> Self {
        let mut registry = NodeRegistry::new();
        node::register_all_nodes(&mut registry);
        let registry = Arc::new(registry);
        let executor = FlowExecutor::new(registry.clone());
        let task_manager = TaskManager::new(executor, registry.clone());
        Self { task_manager, registry }
    }

    /// Create with an empty registry (no built-in nodes)
    pub fn new_empty() -> Self {
        let registry = Arc::new(NodeRegistry::new());
        let executor = FlowExecutor::new(registry.clone());
        let task_manager = TaskManager::new(executor, registry.clone());
        Self { task_manager, registry }
    }

    /// Create with a custom node registry
    pub fn with_registry(registry: NodeRegistry) -> Self {
        let registry = Arc::new(registry);
        let executor = FlowExecutor::new(registry.clone());
        let task_manager = TaskManager::new(executor, registry.clone());
        Self { task_manager, registry }
    }

    /// Dispatch a flow for async execution
    pub fn dispatch(&self, flow: FlowDefinition) -> (TaskId, broadcast::Receiver<TaskCompletion>) {
        self.task_manager.dispatch(flow)
    }

    /// Query the progress of a running task
    pub fn query_progress(&self, task_id: &str) -> Option<TaskProgress> {
        self.task_manager.query_progress(task_id)
    }

    /// Subscribe to completion notifications for a task
    pub fn subscribe(&self, task_id: &str) -> Option<broadcast::Receiver<TaskCompletion>> {
        self.task_manager.subscribe_completion(task_id)
    }

    /// Get the result of a completed task
    pub fn get_result(&self, task_id: &str) -> Option<engine::executor::FlowResult> {
        self.task_manager.get_result(task_id)
    }

    /// Cancel a running task
    pub fn cancel(&self, task_id: &str) -> bool {
        self.task_manager.cancel(task_id)
    }

    /// List all tasks
    pub fn list_tasks(&self) -> Vec<TaskProgress> {
        self.task_manager.list_tasks()
    }

    /// Get the node registry for registering custom node types
    pub fn registry(&self) -> &Arc<NodeRegistry> {
        &self.registry
    }
}

impl Default for ThinkFlow {
    fn default() -> Self {
        Self::new()
    }
}
