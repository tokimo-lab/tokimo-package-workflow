use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::{ReplaceMode, Variable};
use crate::node::traits::NodeResult;
use jsonpath_rust::JsonPath;
use serde_json::Value;
use std::sync::Arc;
use tracing::error;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Assignment,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_assignment(node, ctx, local_vars))),
    );
}

async fn execute_assignment(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Assignment);
    result.status = NodeStatus::Running;

    let assignment_data: AssignmentNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(
                format!("Failed to parse Assignment data: {e}"),
                NodeError::ASSIGNMENT_ERROR,
            );
            return result;
        }
    };

    let mut details: Vec<Value> = Vec::new();
    let mut has_failure = false;

    for item in &assignment_data.assignments {
        let extracted = extract_value(&ctx, &local_vars, &item.method, &item.expression).await;

        match extracted {
            Ok(value) => {
                {
                    let mut var_mgr = ctx.variable.write().await;
                    var_mgr.set(&item.variable, value.clone());
                }

                result.extracted_variables.insert(item.variable.clone(), value.clone());

                details.push(serde_json::json!({
                    "variable": item.variable,
                    "method": item.method,
                    "expression": item.expression,
                    "value": value,
                    "success": true,
                }));
            }
            Err(err_msg) => {
                has_failure = true;
                error!(
                    variable = %item.variable,
                    method = %item.method,
                    error = %err_msg,
                    "Assignment extraction failed"
                );
                details.push(serde_json::json!({
                    "variable": item.variable,
                    "method": item.method,
                    "expression": item.expression,
                    "error": err_msg,
                    "success": false,
                }));
            }
        }
    }

    result.output = Some(serde_json::json!({ "assignments": details }));
    result.detail = Some(serde_json::json!({ "assignments": details }));

    if has_failure {
        let failed_count = details
            .iter()
            .filter(|d| d.get("success") == Some(&Value::Bool(false)))
            .count();
        result.set_error(
            format!("{failed_count} assignment(s) failed"),
            NodeError::ASSIGNMENT_ERROR,
        );
    } else {
        result.status = NodeStatus::Done;
    }

    result
}

async fn extract_value(
    ctx: &ExecutionContext,
    local_vars: &Variable,
    method: &str,
    expression: &str,
) -> Result<Value, String> {
    match method {
        "value" => {
            let var_mgr = ctx.variable.read().await;
            let val = var_mgr.replace_with_local(expression, ReplaceMode::Auto, local_vars);
            Ok(val)
        }
        "jsonpath" => {
            let var_mgr = ctx.variable.read().await;
            let expr = var_mgr.replace_to_string_with_local(expression, local_vars);
            let response_data = var_mgr
                .get_with_local("RESPONSE_DATA", Some(local_vars))
                .unwrap_or(Value::Null);
            drop(var_mgr);

            let json_data = match &response_data {
                Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(response_data.clone()),
                other => other.clone(),
            };

            let results = json_data
                .query(&expr)
                .map_err(|e| format!("Invalid JSONPath '{expr}': {e}"))?;

            match results.len() {
                0 => Ok(Value::Null),
                1 => Ok(results.into_iter().next().unwrap().clone()),
                _ => Ok(Value::Array(results.into_iter().cloned().collect())),
            }
        }
        "regex" => {
            let var_mgr = ctx.variable.read().await;
            let expr = var_mgr.replace_to_string_with_local(expression, local_vars);
            let response_body = var_mgr
                .get_with_local("RESPONSE_BODY", Some(local_vars))
                .map(|v| match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                })
                .unwrap_or_default();
            drop(var_mgr);

            let re = regex::Regex::new(&expr).map_err(|e| format!("Invalid regex '{expr}': {e}"))?;

            if let Some(caps) = re.captures(&response_body) {
                if caps.len() > 1 {
                    Ok(Value::String(
                        caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    ))
                } else {
                    Ok(Value::String(
                        caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    ))
                }
            } else {
                Ok(Value::Null)
            }
        }
        "body" => {
            let var_mgr = ctx.variable.read().await;
            let val = var_mgr
                .get_with_local("RESPONSE_BODY", Some(local_vars))
                .unwrap_or(Value::Null);
            Ok(val)
        }
        "header" => {
            let var_mgr = ctx.variable.read().await;
            let expr = var_mgr.replace_to_string_with_local(expression, local_vars);
            let headers = var_mgr
                .get_with_local("RESPONSE_HEADERS", Some(local_vars))
                .unwrap_or(Value::Null);
            drop(var_mgr);

            match headers {
                Value::Object(map) => {
                    let lower_key = expr.to_lowercase();
                    let val = map
                        .iter()
                        .find(|(k, _)| k.to_lowercase() == lower_key)
                        .map_or(Value::Null, |(_, v)| v.clone());
                    Ok(val)
                }
                _ => Ok(Value::Null),
            }
        }
        "status_code" => {
            let var_mgr = ctx.variable.read().await;
            let val = var_mgr
                .get_with_local("RESPONSE_STATUS", Some(local_vars))
                .unwrap_or(Value::Null);
            Ok(val)
        }
        _ => Err(format!("Unknown assignment method: {method}")),
    }
}
