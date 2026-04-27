use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info};

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Tcp,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_tcp(node, ctx, local_vars))),
    );
}

async fn execute_tcp(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Tcp);
    result.status = NodeStatus::Running;

    let tcp_data: TcpNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse TCP data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Variable replacement
    let var_mgr = ctx.variable.read().await;
    let host = var_mgr.replace_to_string_with_local(&tcp_data.host, &local_vars);
    let send_data = tcp_data
        .data
        .as_ref()
        .map(|d| var_mgr.replace_to_string_with_local(d, &local_vars));
    drop(var_mgr);

    let timeout_ms = tcp_data.timeout.unwrap_or(10000);
    let addr = format!("{}:{}", host, tcp_data.port);

    info!(addr = %addr, "TCP connection");

    // Connect with timeout
    let stream = match tokio::time::timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(addr = %addr, error = %e, "TCP connection failed");
            result.set_error(format!("TCP connection failed: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
        Err(_) => {
            result.set_error(
                format!("TCP connection timed out after {timeout_ms}ms"),
                NodeError::EXECUTE_ERROR,
            );
            return result;
        }
    };

    let (mut reader, mut writer) = stream.into_split();

    // Send data if provided
    if let Some(data) = &send_data {
        if let Err(e) = writer.write_all(data.as_bytes()).await {
            error!(addr = %addr, error = %e, "TCP write failed");
            result.set_error(format!("TCP write failed: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
        let _ = writer.shutdown().await;
    }

    // Read response with timeout
    let mut buf = vec![0u8; 65536];
    let response = match tokio::time::timeout(Duration::from_millis(timeout_ms), reader.read(&mut buf)).await {
        Ok(Ok(n)) => {
            let data = String::from_utf8_lossy(&buf[..n]).to_string();
            Some(data)
        }
        Ok(Err(e)) => {
            // Read error is not fatal if we already sent data
            if send_data.is_none() {
                result.set_error(format!("TCP read failed: {e}"), NodeError::EXECUTE_ERROR);
                return result;
            }
            None
        }
        Err(_) => {
            // Timeout on read is acceptable if we sent data
            None
        }
    };

    let response_str = response.unwrap_or_default();
    let body_value = serde_json::from_str::<serde_json::Value>(&response_str)
        .unwrap_or(serde_json::Value::String(response_str.clone()));

    result.output = Some(serde_json::json!({
        "host": host,
        "port": tcp_data.port,
        "data_sent": send_data.is_some(),
        "response": body_value,
    }));

    result.extra = Some(serde_json::json!({
        "addr": addr,
        "protocol": "tcp",
    }));

    // Set RESPONSE variables
    {
        let mut var_mgr = ctx.variable.write().await;
        var_mgr.set("RESPONSE_BODY", body_value.clone());
        var_mgr.set("RESPONSE_DATA", body_value);
    }

    result.status = NodeStatus::Done;
    result
}
