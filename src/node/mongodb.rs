use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use tracing::info;

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Mongodb,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_mongodb(node, ctx, local_vars))),
    );
}

async fn execute_mongodb(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Mongodb);
    result.status = NodeStatus::Running;

    let mongo_data: MongodbNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse MongoDB data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    let var_mgr = ctx.variable.read().await;
    let command = var_mgr.replace_to_string_with_local(&mongo_data.command, &local_vars);
    let collection = mongo_data
        .collection
        .as_ref()
        .map(|c| var_mgr.replace_to_string_with_local(c, &local_vars));
    drop(var_mgr);

    info!(
        command = %command,
        collection = ?collection,
        server_id = ?mongo_data.server_id,
        connection = ?mongo_data.connection,
        "MongoDB node executed (stub - actual driver not integrated)"
    );

    result.output = Some(serde_json::json!({
        "type": "mongodb",
        "command": command,
        "collection": collection,
        "server_id": mongo_data.server_id,
        "status": "stub_executed",
        "message": "MongoDB driver not yet integrated. Add mongodb dependency for actual execution."
    }));

    result.extra = Some(serde_json::json!({
        "database_type": "mongodb",
        "command": command,
        "collection": collection,
    }));

    {
        let mut var_mgr = ctx.variable.write().await;
        var_mgr.set("RESPONSE_DATA", serde_json::json!([]));
        var_mgr.set("RESPONSE_BODY", serde_json::json!(command));
    }

    result.status = NodeStatus::Done;
    result
}
