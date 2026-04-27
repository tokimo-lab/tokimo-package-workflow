use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{error, info};

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Udp,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_udp(node, ctx, local_vars))),
    );
}

async fn execute_udp(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Udp);
    result.status = NodeStatus::Running;

    let udp_data: UdpNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse UDP data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Variable replacement
    let var_mgr = ctx.variable.read().await;
    let host = var_mgr.replace_to_string_with_local(&udp_data.host, &local_vars);
    let send_data = udp_data
        .data
        .as_ref()
        .map(|d| var_mgr.replace_to_string_with_local(d, &local_vars));
    drop(var_mgr);

    let timeout_ms = udp_data.timeout.unwrap_or(10000);
    let target_addr = format!("{}:{}", host, udp_data.port);

    info!(addr = %target_addr, "UDP communication");

    // Bind to any available local port
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "Failed to bind UDP socket");
            result.set_error(format!("Failed to bind UDP socket: {e}"), NodeError::SYSTEM_ERROR);
            return result;
        }
    };

    // Send data if provided
    if let Some(data) = &send_data {
        match socket.send_to(data.as_bytes(), &target_addr).await {
            Ok(bytes_sent) => {
                info!(addr = %target_addr, bytes = bytes_sent, "UDP data sent");
            }
            Err(e) => {
                error!(addr = %target_addr, error = %e, "UDP send failed");
                result.set_error(format!("UDP send failed: {e}"), NodeError::EXECUTE_ERROR);
                return result;
            }
        }
    }

    // Receive response with timeout
    let mut buf = vec![0u8; 65536];
    let response = match tokio::time::timeout(Duration::from_millis(timeout_ms), socket.recv_from(&mut buf)).await {
        Ok(Ok((n, from_addr))) => {
            let data = String::from_utf8_lossy(&buf[..n]).to_string();
            info!(from = %from_addr, bytes = n, "UDP response received");
            Some(data)
        }
        Ok(Err(e)) => {
            if send_data.is_none() {
                result.set_error(format!("UDP receive failed: {e}"), NodeError::EXECUTE_ERROR);
                return result;
            }
            None
        }
        Err(_) => {
            // Timeout on receive is acceptable if we sent data
            None
        }
    };

    let response_str = response.unwrap_or_default();
    let body_value = serde_json::from_str::<serde_json::Value>(&response_str)
        .unwrap_or(serde_json::Value::String(response_str.clone()));

    result.output = Some(serde_json::json!({
        "host": host,
        "port": udp_data.port,
        "data_sent": send_data.is_some(),
        "response": body_value,
    }));

    result.extra = Some(serde_json::json!({
        "addr": target_addr,
        "protocol": "udp",
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
