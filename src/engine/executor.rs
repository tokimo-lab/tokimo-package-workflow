use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::node::traits::{NodeExecConfig, NodeResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::model::flow::FlowDefinition;

#[derive(Clone)]
pub struct FlowExecutor {
    registry: Arc<NodeRegistry>,
}

impl FlowExecutor {
    pub fn new(registry: Arc<NodeRegistry>) -> Self {
        Self { registry }
    }

    /// Execute a complete flow definition
    pub async fn execute_flow(&self, flow: &FlowDefinition, ctx: &ExecutionContext) -> FlowResult {
        let start = Instant::now();
        let mut results = Vec::new();
        let mut bypass = false;

        info!(flow_name = ?flow.name, "Starting flow execution");

        for (index, node) in flow.nodes.iter().enumerate() {
            let config = NodeExecConfig {
                index,
                bypass,
                ..Default::default()
            };

            let result = self.execute_node(node, ctx, config).await;

            let has_error = result.has_error();
            let is_disabled = node.options.is_disabled();

            results.push(result);

            if has_error && !is_disabled && !ctx.options.ignore_execute_error {
                bypass = true;
                if index + 1 < flow.nodes.len() {
                    warn!(from = index + 1, to = flow.nodes.len(), "Bypassing remaining nodes");
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let status = if results.iter().any(|r| r.status == NodeStatus::Error) {
            FlowStatus::Error
        } else {
            FlowStatus::Done
        };

        info!(?status, elapsed_ms = elapsed, "Flow execution completed");

        FlowResult {
            run_id: ctx.run_id.clone(),
            status,
            results,
            elapsed_ms: elapsed,
            start_time: chrono::Utc::now(),
        }
    }

    /// Execute a single node
    pub async fn execute_node(&self, node: &NodeData, ctx: &ExecutionContext, config: NodeExecConfig) -> NodeResult {
        let node_id = node.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut result = NodeResult::new(node_id.clone(), node.node_type);
        result.parent_id = config.parent_id.clone();
        result.group = config.group;

        // Check disabled
        if node.options.is_disabled() {
            result.status = NodeStatus::Skip;
            info!(
                node_type = ?node.node_type,
                index = config.index,
                "Node disabled, skipping"
            );
            return result;
        }

        // Check bypass
        if config.bypass {
            result.status = NodeStatus::Skip;
            return result;
        }

        // Check skip
        if config.skip {
            result.status = NodeStatus::Skip;
            return result;
        }

        result.status = NodeStatus::Running;

        // Report progress
        {
            let mut output = ctx.output.write().await;
            let node_output = NodeOutput {
                base: BaseOutput {
                    index: config.index,
                    status: result.status,
                    node_type: node.node_type,
                    errors: vec![],
                    total_time: 0.0,
                    timer: NodeTimers::default(),
                },
                detail: None,
                extra: None,
                parent_id: config.parent_id.clone(),
                group: config.group,
                nodes: None,
            };
            output.push(node_output);
        }

        // Execute via registry
        if let Some(executor) = self.registry.get(&node.node_type) {
            let start = Instant::now();
            let exec_result = executor(node.clone(), ctx.clone(), config.local_variables.clone()).await;
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            result.status = exec_result.status;
            result.errors = exec_result.errors;
            result.output = exec_result.output;
            result.extra = exec_result.extra;
            result.detail = exec_result.detail;
            result.children = exec_result.children;
            result.extracted_variables = exec_result.extracted_variables;
            result.timers.execute = Some(elapsed);

            // Merge extracted variables into context
            if !result.extracted_variables.is_empty() {
                let mut var_mgr = ctx.variable.write().await;
                for (k, v) in &result.extracted_variables {
                    var_mgr.set(k, v.clone());
                }
            }
        } else {
            result.set_error(
                format!("No executor registered for node type {:?}", node.node_type),
                NodeError::SYSTEM_ERROR,
            );
            error!(node_type = ?node.node_type, "No executor registered");
        }

        if result.status == NodeStatus::Running {
            result.status = if result.has_error() {
                NodeStatus::Error
            } else {
                NodeStatus::Done
            };
        }

        result
    }
}

/// Status of a completed flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowStatus {
    Wait,
    Running,
    Done,
    Error,
    Skip,
    Cancelled,
}

/// Result of a flow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowResult {
    pub run_id: String,
    pub status: FlowStatus,
    pub results: Vec<NodeResult>,
    pub elapsed_ms: f64,
    pub start_time: chrono::DateTime<chrono::Utc>,
}
