use crate::engine::executor::{FlowExecutor, FlowResult, FlowStatus};
use crate::engine::node_registry::NodeRegistry;
use crate::model::context::{ExecutionContext, ExecutionOptions};
use crate::model::flow::FlowDefinition;
use crate::variable::VariableManager;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tracing::info;
use uuid::Uuid;

/// Unique task identifier
pub type TaskId = String;

/// Current progress of a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub current_node_index: usize,
    pub total_nodes: usize,
    pub current_node_name: Option<String>,
    pub elapsed_ms: f64,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Task lifecycle status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Error,
    Cancelled,
}

/// Internal task state
struct TaskState {
    progress: TaskProgress,
    result: Option<FlowResult>,
    completion_tx: broadcast::Sender<TaskCompletion>,
    progress_tx: watch::Sender<TaskProgress>,
}

/// Completion notification payload
#[derive(Debug, Clone)]
pub struct TaskCompletion {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub result: FlowResult,
}

/// Manages task dispatch, progress tracking, and completion notification
pub struct TaskManager {
    executor: FlowExecutor,
    registry: Arc<NodeRegistry>,
    tasks: Arc<DashMap<TaskId, TaskState>>,
}

impl TaskManager {
    pub fn new(executor: FlowExecutor, registry: Arc<NodeRegistry>) -> Self {
        Self {
            executor,
            registry,
            tasks: Arc::new(DashMap::new()),
        }
    }

    /// Dispatch a flow for execution, returns the task ID immediately.
    pub fn dispatch(&self, flow: FlowDefinition) -> (TaskId, broadcast::Receiver<TaskCompletion>) {
        let task_id = Uuid::new_v4().to_string();
        let (completion_tx, completion_rx) = broadcast::channel(1);

        let now = Utc::now();
        let progress = TaskProgress {
            task_id: task_id.clone(),
            status: TaskStatus::Pending,
            current_node_index: 0,
            total_nodes: flow.nodes.len(),
            current_node_name: None,
            elapsed_ms: 0.0,
            started_at: now,
            completed_at: None,
        };

        let (progress_tx, _) = watch::channel(progress.clone());

        let state = TaskState {
            progress,
            result: None,
            completion_tx: completion_tx.clone(),
            progress_tx: progress_tx.clone(),
        };

        self.tasks.insert(task_id.clone(), state);

        // Spawn background task
        let executor = self.executor.clone();
        let registry = self.registry.clone();
        let tasks = self.tasks.clone();
        let tid = task_id.clone();

        tokio::spawn(async move {
            info!(task_id = %tid, "Task started");

            // Update status to running
            if let Some(mut state) = tasks.get_mut(&tid) {
                state.progress.status = TaskStatus::Running;
                let _ = state.progress_tx.send(state.progress.clone());
            }

            // Build context
            let mut var_mgr = VariableManager::new();
            if let Some(vars) = &flow.variables {
                for (k, v) in vars {
                    var_mgr.set_scope(crate::model::variable::VariableScope::Execute, k, v.clone());
                }
            }

            let options = ExecutionOptions {
                ignore_execute_error: false,
                stdout: true,
                max_async_coroutines: 10,
            };

            let ctx = ExecutionContext::new(tid.clone(), var_mgr, options, registry.clone());

            // Execute
            let result = executor.execute_flow(&flow, &ctx).await;

            let status = match result.status {
                FlowStatus::Error => TaskStatus::Error,
                _ => TaskStatus::Done,
            };

            // Update final state
            if let Some(mut state) = tasks.get_mut(&tid) {
                state.progress.status = status;
                state.progress.elapsed_ms = result.elapsed_ms;
                state.progress.completed_at = Some(Utc::now());
                let _ = state.progress_tx.send(state.progress.clone());

                let completion = TaskCompletion {
                    task_id: tid.clone(),
                    status,
                    result: result.clone(),
                };
                let _ = state.completion_tx.send(completion);
                state.result = Some(result);
            }

            info!(task_id = %tid, ?status, "Task completed");
        });

        (task_id, completion_rx)
    }

    /// Query the current progress of a task
    pub fn query_progress(&self, task_id: &str) -> Option<TaskProgress> {
        self.tasks.get(task_id).map(|state| state.progress.clone())
    }

    /// Subscribe to progress updates for a task
    pub fn subscribe_progress(&self, task_id: &str) -> Option<watch::Receiver<TaskProgress>> {
        self.tasks.get(task_id).map(|state| state.progress_tx.subscribe())
    }

    /// Subscribe to completion notification for a task
    pub fn subscribe_completion(&self, task_id: &str) -> Option<broadcast::Receiver<TaskCompletion>> {
        self.tasks.get(task_id).map(|state| state.completion_tx.subscribe())
    }

    /// Get the result of a completed task
    pub fn get_result(&self, task_id: &str) -> Option<FlowResult> {
        self.tasks.get(task_id).and_then(|state| state.result.clone())
    }

    /// Cancel a running task
    pub fn cancel(&self, task_id: &str) -> bool {
        if let Some(mut state) = self.tasks.get_mut(task_id)
            && (state.progress.status == TaskStatus::Running || state.progress.status == TaskStatus::Pending)
        {
            state.progress.status = TaskStatus::Cancelled;
            let _ = state.progress_tx.send(state.progress.clone());
            return true;
        }
        false
    }

    /// List all task IDs
    pub fn list_tasks(&self) -> Vec<TaskProgress> {
        self.tasks.iter().map(|entry| entry.progress.clone()).collect()
    }

    /// Remove a completed task from memory
    pub fn remove(&self, task_id: &str) -> bool {
        self.tasks.remove(task_id).is_some()
    }
}
