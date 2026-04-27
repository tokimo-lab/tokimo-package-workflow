use crate::engine::executor::FlowExecutor;
use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::{NodeExecConfig, NodeResult};
use std::sync::Arc;
use tracing::info;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Condition,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_condition(node, ctx, local_vars))),
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

async fn execute_condition(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id.clone(), NodeType::Condition);
    result.status = NodeStatus::Running;

    let cond_data: ConditionNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse condition data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Evaluate condition with variable replacement using Syntax mode
    let condition_result = {
        let var_mgr = ctx.variable.read().await;
        let replaced = match var_mgr
            .replace_with_local(
                &cond_data.condition,
                crate::model::variable::ReplaceMode::Syntax,
                &local_vars,
            )
            .as_str()
        {
            Some(s) => s.to_string(),
            None => var_mgr.replace_to_string_with_local(&cond_data.condition, &local_vars),
        };

        let engine = rhai::Engine::new();
        match engine.eval_expression::<rhai::Dynamic>(&replaced) {
            Ok(val) => is_truthy(&val),
            Err(_) => matches!(replaced.trim(), "true" | "1"),
        }
    };

    let branch_index = usize::from(!condition_result);
    info!(
        condition = %cond_data.condition,
        result = condition_result,
        branch = branch_index,
        "Condition evaluated"
    );

    let executor = FlowExecutor::new(ctx.registry.clone());
    let mut all_children = Vec::new();

    for (group_idx, group_nodes) in cond_data.nodes.iter().enumerate() {
        let mut group_results = Vec::new();
        let is_active = group_idx == branch_index;
        let mut bypass = false;

        for (idx, child_node) in group_nodes.iter().enumerate() {
            let config = NodeExecConfig {
                index: idx,
                group: Some(group_idx),
                deep: 1,
                parent_id: Some(node_id.clone()),
                skip: !is_active,
                bypass: is_active && bypass,
                local_variables: local_vars.clone(),
            };

            let child_result = executor.execute_node(child_node, &ctx, config).await;
            if child_result.has_error() && !child_node.options.is_disabled() {
                bypass = true;
            }
            group_results.push(child_result);
        }
        all_children.push(group_results);
    }

    let has_error = all_children
        .get(branch_index)
        .is_some_and(|branch| branch.iter().any(super::traits::NodeResult::has_error));

    result.status = if has_error { NodeStatus::Error } else { NodeStatus::Done };
    result.extra = Some(serde_json::json!({
        "condition": cond_data.condition,
        "result": condition_result,
        "branch": branch_index,
    }));
    result.children = Some(all_children);
    result
}
