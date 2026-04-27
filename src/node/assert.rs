use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::{ReplaceMode, Variable};
use crate::node::traits::NodeResult;
use serde_json::Value;
use std::sync::Arc;
use tracing::error;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Assert,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_assert(node, ctx, local_vars))),
    );
}

async fn execute_assert(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Assert);
    result.status = NodeStatus::Running;

    let assert_data: AssertNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse Assert data: {e}"), NodeError::VALIDATOR_ERROR);
            return result;
        }
    };

    let var_mgr = ctx.variable.read().await;
    let mut details: Vec<Value> = Vec::new();
    let mut has_failure = false;

    for item in &assert_data.asserts {
        let source_val = var_mgr.replace_with_local(&item.source, ReplaceMode::Auto, &local_vars);

        let expect_val = item.expect.as_ref().map(|e| {
            if let Value::String(s) = e {
                var_mgr.replace_with_local(s, ReplaceMode::Auto, &local_vars)
            } else {
                e.clone()
            }
        });

        let passed = check_assertion(&source_val, &item.method, &expect_val);

        details.push(serde_json::json!({
            "source": item.source,
            "method": item.method,
            "expect": expect_val,
            "actual": source_val,
            "passed": passed,
        }));

        if !passed {
            has_failure = true;
        }
    }

    drop(var_mgr);

    let output = serde_json::json!({
        "asserts": details,
        "passed": !has_failure,
    });
    result.output = Some(output);
    result.detail = Some(serde_json::json!({ "asserts": details }));

    if has_failure {
        let failed_count = details
            .iter()
            .filter(|d| d.get("passed") == Some(&Value::Bool(false)))
            .count();
        error!(failed_count, "Assertion(s) failed");
        result.set_error(
            format!("{failed_count} assertion(s) failed"),
            NodeError::VALIDATOR_ERROR,
        );
    } else {
        result.status = NodeStatus::Done;
    }

    result
}

fn check_assertion(source: &Value, method: &str, expect: &Option<Value>) -> bool {
    match method {
        "eq" => {
            if let Some(exp) = expect {
                values_equal(source, exp)
            } else {
                source.is_null()
            }
        }
        "ne" => {
            if let Some(exp) = expect {
                !values_equal(source, exp)
            } else {
                !source.is_null()
            }
        }
        "gt" => numeric_compare(source, expect, |a, b| a > b),
        "lt" => numeric_compare(source, expect, |a, b| a < b),
        "gte" => numeric_compare(source, expect, |a, b| a >= b),
        "lte" => numeric_compare(source, expect, |a, b| a <= b),
        "contains" => {
            let source_str = value_to_string(source);
            let expect_str = expect.as_ref().map(value_to_string).unwrap_or_default();
            source_str.contains(&expect_str)
        }
        "not_contains" => {
            let source_str = value_to_string(source);
            let expect_str = expect.as_ref().map(value_to_string).unwrap_or_default();
            !source_str.contains(&expect_str)
        }
        "regex" => {
            let source_str = value_to_string(source);
            let pattern = expect.as_ref().map(value_to_string).unwrap_or_default();
            regex::Regex::new(&pattern).is_ok_and(|re| re.is_match(&source_str))
        }
        "type" => {
            let expected_type = expect.as_ref().map(value_to_string).unwrap_or_default();
            check_type(source, &expected_type)
        }
        "empty" => is_empty(source),
        "not_empty" => !is_empty(source),
        _ => false,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    if let (Some(na), Some(nb)) = (to_f64(a), to_f64(b)) {
        return (na - nb).abs() < f64::EPSILON;
    }
    a == b
}

fn to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn numeric_compare(source: &Value, expect: &Option<Value>, cmp: fn(f64, f64) -> bool) -> bool {
    let a = to_f64(source);
    let b = expect.as_ref().and_then(to_f64);
    match (a, b) {
        (Some(va), Some(vb)) => cmp(va, vb),
        _ => false,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn check_type(source: &Value, expected_type: &str) -> bool {
    match expected_type {
        "string" => source.is_string(),
        "number" => source.is_number(),
        "boolean" | "bool" => source.is_boolean(),
        "null" => source.is_null(),
        "array" => source.is_array(),
        "object" => source.is_object(),
        _ => false,
    }
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}
