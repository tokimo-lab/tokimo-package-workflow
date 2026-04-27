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
        NodeType::DataLoop,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_data_loop(node, ctx, local_vars))),
    );
}

async fn execute_data_loop(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id.clone(), NodeType::DataLoop);
    result.status = NodeStatus::Running;

    let loop_data: DataLoopNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse data loop data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    let config = loop_data.config.unwrap_or(LoopConfig {
        index_var: "index".into(),
        r#async: false,
        ignore_error: false,
    });

    let data_len = loop_data.data.len();
    info!(data_len = data_len, async_mode = config.r#async, "DataLoop starting");

    let executor = FlowExecutor::new(ctx.registry.clone());
    let mut all_children = Vec::new();
    let mut has_error = false;

    if config.r#async {
        let max_concurrent = ctx.options.max_async_coroutines.max(1);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for (i, item) in loop_data.data.iter().enumerate() {
            let executor = executor.clone();
            let ctx = ctx.clone();
            let node_id = node_id.clone();
            let mut iter_local = local_vars.clone();
            iter_local.insert(config.index_var.clone(), Value::Number(i.into()));

            // Spread data item fields into local variables
            if let Value::Object(map) = item {
                for (k, v) in map {
                    iter_local.insert(k.clone(), v.clone());
                }
            }
            iter_local.insert("item".into(), item.clone());

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
                    warn!(error = %e, "DataLoop iteration task failed");
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
        for (i, item) in loop_data.data.iter().enumerate() {
            let mut iter_local = local_vars.clone();
            iter_local.insert(config.index_var.clone(), Value::Number(i.into()));

            if let Value::Object(map) = item {
                for (k, v) in map {
                    iter_local.insert(k.clone(), v.clone());
                }
            }
            iter_local.insert("item".into(), item.clone());

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
        "data_length": data_len,
        "iterations": all_children.len(),
    }));
    result.children = Some(all_children);
    result
}
