use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use tracing::info;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Grpc,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_grpc(node, ctx, local_vars))),
    );
}

#[allow(clippy::unused_async)]
async fn execute_grpc(node: NodeData, _ctx: ExecutionContext, _local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Grpc);
    result.status = NodeStatus::Running;

    let grpc_data: GrpcNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse gRPC data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    info!(
        host = %grpc_data.host,
        port = grpc_data.port,
        service = %grpc_data.service,
        method = %grpc_data.method,
        "gRPC call (stub)"
    );

    result.output = Some(serde_json::json!({
        "message": "gRPC not yet fully implemented - architecture placeholder",
        "host": grpc_data.host,
        "port": grpc_data.port,
        "service": grpc_data.service,
        "method": grpc_data.method,
    }));

    result.extra = Some(serde_json::json!({
        "protocol": "grpc",
        "stub": true,
    }));

    result.status = NodeStatus::Done;
    result
}
