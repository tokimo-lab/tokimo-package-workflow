use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Http,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_http(node, ctx, local_vars))),
    );
}

async fn execute_http(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Http);
    result.status = NodeStatus::Running;

    let http_data: HttpNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse HTTP data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Variable replacement
    let var_mgr = ctx.variable.read().await;
    let method = var_mgr.replace_to_string_with_local(&http_data.method, &local_vars);
    let path = var_mgr.replace_to_string_with_local(&http_data.path, &local_vars);
    let protocol = var_mgr.replace_to_string_with_local(&http_data.protocol, &local_vars);
    let hostname = http_data
        .hostname
        .as_ref()
        .map(|h| var_mgr.replace_to_string_with_local(h, &local_vars))
        .unwrap_or_default();
    drop(var_mgr);

    // Build URL
    let port_str = http_data.port.map(|p| format!(":{p}")).unwrap_or_default();
    let url = format!("{protocol}://{hostname}{port_str}{path}");

    // Build query params
    let mut query_params = Vec::new();
    if let Some(params) = &http_data.params {
        let var_mgr = ctx.variable.read().await;
        for p in params {
            let key = var_mgr.replace_to_string_with_local(&p.key, &local_vars);
            let val = var_mgr.replace_to_string_with_local(&p.value, &local_vars);
            query_params.push((key, val));
        }
    }

    // Config
    let config = http_data.config.unwrap_or_default();
    let timeout_ms = config.timeout.unwrap_or(30000);
    let retry = config.retry.unwrap_or(0);
    let follow_redirect = config.follow_redirect.unwrap_or(true);

    // Build client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(if follow_redirect {
            reqwest::redirect::Policy::limited(config.max_redirects.unwrap_or(10) as usize)
        } else {
            reqwest::redirect::Policy::none()
        })
        .build()
        .unwrap_or_default();

    // Build request method
    let req_method = method
        .to_uppercase()
        .parse::<reqwest::Method>()
        .unwrap_or(reqwest::Method::GET);

    // Execute with retry
    let mut last_error = None;
    let total_attempts = retry + 1;

    for attempt in 0..total_attempts {
        info!(url = %url, method = %req_method, attempt = attempt + 1, "HTTP request");

        // Rebuild request each attempt for retry support
        let mut req = client.request(req_method.clone(), &url);
        if !query_params.is_empty() {
            req = req.query(&query_params);
        }
        if let Some(headers) = &http_data.headers {
            let var_mgr = ctx.variable.read().await;
            for h in headers {
                let key = var_mgr.replace_to_string_with_local(&h.key, &local_vars);
                let val = var_mgr.replace_to_string_with_local(&h.value, &local_vars);
                req = req.header(key, val);
            }
        }
        if let Some(body) = &http_data.body {
            let var_mgr = ctx.variable.read().await;
            let body_str = match body {
                serde_json::Value::String(s) => var_mgr.replace_to_string_with_local(s, &local_vars),
                other => {
                    let s = serde_json::to_string(other).unwrap_or_default();
                    var_mgr.replace_to_string_with_local(&s, &local_vars)
                }
            };
            drop(var_mgr);
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                req = req.json(&json_val);
            } else {
                req = req.body(body_str);
            }
        }
        if let Some(form) = &http_data.form {
            let var_mgr = ctx.variable.read().await;
            let mut form_data = Vec::new();
            for f in form {
                let key = var_mgr.replace_to_string_with_local(&f.key, &local_vars);
                let val = var_mgr.replace_to_string_with_local(&f.value, &local_vars);
                form_data.push((key, val));
            }
            drop(var_mgr);
            req = req.form(&form_data);
        }

        match req.send().await {
            Ok(response) => {
                let status_code = response.status().as_u16();
                let headers: serde_json::Map<String, serde_json::Value> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
                        )
                    })
                    .collect();

                let body = response.text().await.unwrap_or_default();

                let body_value =
                    serde_json::from_str::<serde_json::Value>(&body).unwrap_or(serde_json::Value::String(body));

                result.output = Some(serde_json::json!({
                    "status_code": status_code,
                    "headers": headers,
                    "body": body_value,
                }));

                result.extra = Some(serde_json::json!({
                    "url": url,
                    "method": req_method.to_string(),
                    "status_code": status_code,
                    "attempt": attempt + 1,
                }));

                // Set RESPONSE variables for downstream nodes
                {
                    let mut var_mgr = ctx.variable.write().await;
                    var_mgr.set("RESPONSE_STATUS", serde_json::json!(status_code));
                    var_mgr.set("RESPONSE_BODY", body_value.clone());
                    var_mgr.set("RESPONSE_HEADERS", serde_json::json!(headers));
                    var_mgr.set("RESPONSE_DATA", body_value);
                }

                result.status = NodeStatus::Done;
                return result;
            }
            Err(e) => {
                error!(url = %url, attempt = attempt + 1, error = %e, "HTTP request failed");
                last_error = Some(format!("HTTP error: {e}"));
                if attempt + 1 < total_attempts {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    if let Some(err) = last_error {
        result.set_error(err, NodeError::EXECUTE_ERROR);
    }
    result
}
