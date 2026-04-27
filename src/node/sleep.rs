use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Sleep,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_sleep(node, ctx, local_vars))),
    );
}

fn resolve_time_ms(val: &Value, var_mgr: &crate::variable::VariableManager, local: &Variable) -> u64 {
    match val {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => {
            let replaced = var_mgr.replace_to_string_with_local(s, local);
            replaced.trim().parse::<u64>().unwrap_or(0)
        }
        _ => 0,
    }
}

async fn execute_sleep(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Sleep);
    result.status = NodeStatus::Running;

    let sleep_data: SleepNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse sleep data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    let ms = {
        let var_mgr = ctx.variable.read().await;
        resolve_time_ms(&sleep_data.time, &var_mgr, &local_vars)
    };

    info!(duration_ms = ms, "Sleep starting");
    tokio::time::sleep(Duration::from_millis(ms)).await;
    info!(duration_ms = ms, "Sleep completed");

    result.status = NodeStatus::Done;
    result.extra = Some(serde_json::json!({ "time": ms }));
    result
}
