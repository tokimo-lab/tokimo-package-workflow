use crate::engine::executor::FlowExecutor;
use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::{NodeExecConfig, NodeResult};
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, warn};

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Loop,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_loop(node, ctx, local_vars))),
    );
}

fn resolve_count(val: &Value, var_mgr: &crate::variable::VariableManager, local: &Variable) -> usize {
    match val {
        Value::Number(n) => n.as_u64().unwrap_or(0) as usize,
        Value::String(s) => {
            let replaced = var_mgr.replace_to_string_with_local(s, local);
            replaced.trim().parse::<usize>().unwrap_or(0)
        }
        _ => 0,
    }
}

async fn execute_loop(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id.clone(), NodeType::Loop);
    result.status = NodeStatus::Running;

    let loop_data: LoopNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse loop data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    let count = {
        let var_mgr = ctx.variable.read().await;
        resolve_count(&loop_data.count, &var_mgr, &local_vars)
    };

    let config = loop_data.config.unwrap_or(LoopConfig {
        index_var: "index".into(),
        r#async: false,
        ignore_error: false,
    });

    info!(count = count, async_mode = config.r#async, "Loop starting");

    let executor = FlowExecutor::new(ctx.registry.clone());
    let mut all_children = Vec::new();
    let mut has_error = false;

    if config.r#async {
        // Concurrent execution with semaphore
        let max_concurrent = ctx.options.max_async_coroutines.max(1);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for i in 0..count {
            let executor = executor.clone();
            let ctx = ctx.clone();
            let node_id = node_id.clone();
            let mut iter_local = local_vars.clone();
            iter_local.insert(config.index_var.clone(), Value::Number(i.into()));

            // Spread data item fields into local vars if present
            if let Some(ref data) = loop_data.data
                && let Some(item) = data.get(i)
                && let Value::Object(map) = item
            {
                for (k, v) in map {
                    iter_local.insert(k.clone(), v.clone());
                }
            }

            let nodes = loop_data.nodes.clone();
            let sem = semaphore.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let mut group_results = Vec::new();
                let mut bypass = false;

                for (idx, child_node) in nodes.iter().enumerate() {
                    let child_config = NodeExecConfig {
                        index: idx,
                        group: Some(i),
                        deep: 1,
                        parent_id: Some(node_id.clone()),
                        skip: false,
                        bypass,
                        local_variables: iter_local.clone(),
                    };

                    let child_result = executor.execute_node(child_node, &ctx, child_config).await;
                    if child_result.has_error() && !child_node.options.is_disabled() {
                        bypass = true;
                    }
                    group_results.push(child_result);
                }
                (i, group_results)
            }));
        }

        let mut indexed_results: Vec<(usize, Vec<NodeResult>)> = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(r) => indexed_results.push(r),
                Err(e) => {
                    warn!(error = %e, "Loop iteration task failed");
                    has_error = true;
                }
            }
        }
        indexed_results.sort_by_key(|(i, _)| *i);
        for (_, results) in indexed_results {
            if results.iter().any(super::traits::NodeResult::has_error) {
                has_error = true;
            }
            all_children.push(results);
        }
    } else {
        // Sequential execution
        for i in 0..count {
            let mut iter_local = local_vars.clone();
            iter_local.insert(config.index_var.clone(), Value::Number(i.into()));

            if let Some(ref data) = loop_data.data
                && let Some(item) = data.get(i)
                && let Value::Object(map) = item
            {
                for (k, v) in map {
                    iter_local.insert(k.clone(), v.clone());
                }
            }

            let mut group_results = Vec::new();
            let mut bypass = false;

            for (idx, child_node) in loop_data.nodes.iter().enumerate() {
                let child_config = NodeExecConfig {
                    index: idx,
                    group: Some(i),
                    deep: 1,
                    parent_id: Some(node_id.clone()),
                    skip: false,
                    bypass,
                    local_variables: iter_local.clone(),
                };

                let child_result = executor.execute_node(child_node, &ctx, child_config).await;
                if child_result.has_error() && !child_node.options.is_disabled() {
                    bypass = true;
                }
                group_results.push(child_result);
            }

            if group_results.iter().any(super::traits::NodeResult::has_error) {
                has_error = true;
                if !config.ignore_error {
                    all_children.push(group_results);
                    break;
                }
            }
            all_children.push(group_results);
        }
    }

    result.status = if has_error { NodeStatus::Error } else { NodeStatus::Done };
    result.extra = Some(serde_json::json!({
        "count": count,
        "iterations": all_children.len(),
    }));
    result.children = Some(all_children);
    result
}
