use crate::engine::node_registry::NodeRegistry;

// Import registration functions from each node module
use super::assert::register as register_assert;
use super::assignment::register as register_assignment;
use super::condition::register as register_condition;
use super::data_loop::register as register_data_loop;
use super::grpc::register as register_grpc;
use super::http::register as register_http;
use super::loop_node::register as register_loop;
use super::mongodb::register as register_mongodb;
use super::mssql::register as register_mssql;
use super::mysql::register as register_mysql;
use super::poll::register as register_poll;
use super::postgresql::register as register_postgresql;
use super::redis::register as register_redis;
use super::script::register as register_script;
use super::shell::register as register_shell;
use super::sleep::register as register_sleep;
use super::tcp::register as register_tcp;
use super::udp::register as register_udp;

/// Register all built-in node types with the registry
pub fn register_all_nodes(registry: &mut NodeRegistry) {
    // Flow control
    register_condition(registry);
    register_loop(registry);
    register_data_loop(registry);
    register_sleep(registry);
    register_poll(registry);
    // Protocols
    register_http(registry);
    register_shell(registry);
    register_tcp(registry);
    register_udp(registry);
    register_grpc(registry);
    // Components
    register_script(registry);
    register_assert(registry);
    register_assignment(registry);
    // Databases
    register_mysql(registry);
    register_postgresql(registry);
    register_mongodb(registry);
    register_redis(registry);
    register_mssql(registry);
}
