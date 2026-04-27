use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use serde_json::Value;
use std::sync::Arc;
use tracing::error;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Script,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_script(node, ctx, local_vars))),
    );
}

async fn execute_script(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Script);
    result.status = NodeStatus::Running;

    let script_data: ScriptNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse Script data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Variable replacement on script content
    let script = {
        let var_mgr = ctx.variable.read().await;
        var_mgr.replace_to_string_with_local(&script_data.script, &local_vars)
    };

    let timeout_ms = script_data.timeout.unwrap_or(30000);

    // Snapshot current variables for the rhai engine
    let var_snapshot: Vec<(String, Value)> = {
        let var_mgr = ctx.variable.read().await;
        let scope = var_mgr.get_scope(crate::model::variable::VariableScope::Context);
        scope.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };

    // Channel for sys_set calls from rhai
    let (set_tx, mut set_rx) = tokio::sync::mpsc::unbounded_channel::<(String, Value)>();
    // Shared map for sys_get (read-only snapshot + accumulated sets)
    let get_store = Arc::new(std::sync::RwLock::new(
        var_snapshot
            .into_iter()
            .collect::<std::collections::HashMap<String, Value>>(),
    ));

    let script_clone = script.clone();
    let get_store_clone = get_store.clone();
    let set_tx_clone = set_tx.clone();

    let eval_task = tokio::task::spawn_blocking(move || {
        let mut engine = rhai::Engine::new();

        // Resource limits to prevent runaway scripts
        engine.set_max_operations(1_000_000);
        engine.set_max_call_levels(64);
        engine.set_max_string_size(10 * 1024 * 1024); // 10 MB
        engine.set_max_array_size(100_000);
        engine.set_max_map_size(100_000);

        // Register sys_get(key) -> Dynamic
        let gs = get_store_clone.clone();
        engine.register_fn("sys_get", move |key: &str| -> rhai::Dynamic {
            let store = gs.read().unwrap();
            match store.get(key) {
                Some(val) => json_to_dynamic(val),
                None => rhai::Dynamic::UNIT,
            }
        });

        // Register sys_set(key, val)
        let tx = set_tx_clone.clone();
        let gs2 = get_store_clone;
        engine.register_fn("sys_set", move |key: &str, val: rhai::Dynamic| {
            let json_val = dynamic_to_json(&val);
            if let Ok(mut store) = gs2.write() {
                store.insert(key.to_string(), json_val.clone());
            }
            let _ = tx.send((key.to_string(), json_val));
        });

        engine.eval::<rhai::Dynamic>(&script_clone)
    });

    let eval_result = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), eval_task).await;

    // Process sys_set calls
    drop(set_tx);
    {
        let mut var_mgr = ctx.variable.write().await;
        while let Ok((key, val)) = set_rx.try_recv() {
            var_mgr.set(&key, val);
        }
    }

    match eval_result {
        Ok(Ok(Ok(dynamic_val))) => {
            let output = dynamic_to_json(&dynamic_val);
            result.output = Some(output.clone());
            result.extra = Some(serde_json::json!({ "script": script }));

            {
                let mut var_mgr = ctx.variable.write().await;
                var_mgr.set("SCRIPT_RESULT", output);
            }

            result.status = NodeStatus::Done;
        }
        Ok(Ok(Err(eval_err))) => {
            error!(error = %eval_err, "Rhai script execution error");
            result.set_error(format!("Script error: {eval_err}"), NodeError::EXECUTE_ERROR);
        }
        Ok(Err(join_err)) => {
            error!(error = %join_err, "Script task panicked");
            result.set_error(format!("Script task error: {join_err}"), NodeError::EXECUTE_ERROR);
        }
        Err(_) => {
            error!(timeout_ms = timeout_ms, "Script execution timed out");
            result.set_error(
                format!("Script timed out after {timeout_ms}ms"),
                NodeError::EXECUTE_ERROR,
            );
        }
    }

    result
}

fn json_to_dynamic(val: &Value) -> rhai::Dynamic {
    match val {
        Value::Null => rhai::Dynamic::UNIT,
        Value::Bool(b) => rhai::Dynamic::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rhai::Dynamic::from(i)
            } else if let Some(f) = n.as_f64() {
                rhai::Dynamic::from(f)
            } else {
                rhai::Dynamic::from(n.to_string())
            }
        }
        Value::String(s) => rhai::Dynamic::from(s.clone()),
        Value::Array(arr) => {
            let items: Vec<rhai::Dynamic> = arr.iter().map(json_to_dynamic).collect();
            rhai::Dynamic::from(items)
        }
        Value::Object(map) => {
            let mut rhai_map = rhai::Map::new();
            for (k, v) in map {
                rhai_map.insert(k.clone().into(), json_to_dynamic(v));
            }
            rhai::Dynamic::from(rhai_map)
        }
    }
}

fn dynamic_to_json(val: &rhai::Dynamic) -> Value {
    if val.is_unit() {
        Value::Null
    } else if val.is_bool() {
        Value::Bool(val.as_bool().unwrap_or(false))
    } else if val.is_int() {
        Value::Number(serde_json::Number::from(val.as_int().unwrap_or(0)))
    } else if val.is_float() {
        let f = val.as_float().unwrap_or(0.0);
        serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
    } else if val.is_string() {
        Value::String(val.clone().into_string().unwrap_or_default())
    } else if val.is_array() {
        let arr = val.clone().into_typed_array::<rhai::Dynamic>().unwrap_or_default();
        Value::Array(arr.iter().map(dynamic_to_json).collect())
    } else if val.is_map() {
        let map = val.clone().cast::<rhai::Map>();
        let obj: serde_json::Map<String, Value> =
            map.iter().map(|(k, v)| (k.to_string(), dynamic_to_json(v))).collect();
        Value::Object(obj)
    } else {
        Value::String(val.to_string())
    }
}
