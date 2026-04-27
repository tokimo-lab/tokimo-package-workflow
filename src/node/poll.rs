use crate::engine::executor::FlowExecutor;
use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::{NodeExecConfig, NodeResult};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Poll,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_poll(node, ctx, local_vars))),
    );
}

fn is_truthy(val: &rhai::Dynamic) -> bool {
    if let Ok(b) = val.as_bool() {
        return b;
    }
    if let Ok(i) = val.as_int() {
        return i != 0;
    }
    if let Ok(f) = val.as_float() {
        return f != 0.0;
    }
    if let Ok(s) = val.clone().into_string() {
        return !s.is_empty();
    }
    true
}

async fn execute_poll(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id.clone(), NodeType::Poll);
    result.status = NodeStatus::Running;

    let poll_data: PollNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse poll data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    info!(
        max_attempts = poll_data.max_attempts,
        interval_ms = poll_data.interval,
        condition = %poll_data.condition,
        "Poll starting"
    );

    let executor = FlowExecutor::new(ctx.registry.clone());
    let mut all_children = Vec::new();
    let mut condition_met = false;

    for attempt in 0..poll_data.max_attempts {
        // Execute child nodes for this attempt
        let mut group_results = Vec::new();
        let mut bypass = false;

        for (idx, child_node) in poll_data.nodes.iter().enumerate() {
            let config = NodeExecConfig {
                index: idx,
                group: Some(attempt as usize),
                deep: 1,
                parent_id: Some(node_id.clone()),
                skip: false,
                bypass,
                local_variables: local_vars.clone(),
            };

            let child_result = executor.execute_node(child_node, &ctx, config).await;
            if child_result.has_error() && !child_node.options.is_disabled() {
                bypass = true;
            }
            group_results.push(child_result);
        }

        all_children.push(group_results);

        // Evaluate the stop condition
        let should_stop = {
            let var_mgr = ctx.variable.read().await;
            let replaced = match var_mgr
                .replace_with_local(
                    &poll_data.condition,
                    crate::model::variable::ReplaceMode::Syntax,
                    &local_vars,
                )
                .as_str()
            {
                Some(s) => s.to_string(),
                None => var_mgr.replace_to_string_with_local(&poll_data.condition, &local_vars),
            };

            let engine = rhai::Engine::new();
            match engine.eval_expression::<rhai::Dynamic>(&replaced) {
                Ok(val) => is_truthy(&val),
                Err(_) => matches!(replaced.trim(), "true" | "1"),
            }
        };

        if should_stop {
            info!(attempt = attempt + 1, "Poll condition met");
            condition_met = true;
            break;
        }

        // Wait before next attempt (don't wait after the last attempt)
        if attempt + 1 < poll_data.max_attempts {
            tokio::time::sleep(Duration::from_millis(poll_data.interval)).await;
        }
    }

    if !condition_met {
        result.set_error(
            format!("Poll condition not met after {} attempts", poll_data.max_attempts),
            NodeError::EXECUTE_ERROR,
        );
    }

    result.status = if result.has_error() {
        NodeStatus::Error
    } else {
        NodeStatus::Done
    };
    result.extra = Some(serde_json::json!({
        "condition": poll_data.condition,
        "max_attempts": poll_data.max_attempts,
        "attempts": all_children.len(),
        "condition_met": condition_met,
    }));
    result.children = Some(all_children);
    result
}
