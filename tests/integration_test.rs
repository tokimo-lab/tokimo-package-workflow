use std::collections::HashMap;

use serde_json::json;
use tokimo_package_workflow::ThinkFlow;
use tokimo_package_workflow::engine::executor::FlowStatus;
use tokimo_package_workflow::engine::task_manager::TaskStatus;
use tokimo_package_workflow::model::flow::FlowDefinition;
use tokimo_package_workflow::model::node::*;
use tokimo_package_workflow::model::variable::Variable;
use tokimo_package_workflow::node::traits::NodeResult;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_node(node_type: NodeType, data: serde_json::Value) -> NodeData {
    NodeData {
        node_type,
        id: None,
        name: None,
        remark: None,
        options: NodeOptions(0),
        scripts: None,
        asserts: None,
        assignments: None,
        data,
    }
}

fn simple_flow(nodes: Vec<NodeData>) -> FlowDefinition {
    FlowDefinition {
        name: Some("Test Flow".into()),
        id: Some("test-1".into()),
        r#async: false,
        timeout: Some(30000),
        nodes,
        variables: None,
        options: None,
    }
}

fn flow_with_vars(nodes: Vec<NodeData>, vars: Variable) -> FlowDefinition {
    FlowDefinition {
        name: Some("Var Test Flow".into()),
        id: None,
        r#async: false,
        timeout: None,
        nodes,
        variables: Some(vars),
        options: None,
    }
}

/// Helper to find the first NodeResult with a given NodeType in a flat results list.
fn find_result_by_type(results: &[NodeResult], nt: NodeType) -> Option<&NodeResult> {
    results.iter().find(|r| r.node_type == nt)
}

// ===========================================================================
// 1. Dispatch a flow and get a task ID
// ===========================================================================

#[tokio::test]
async fn test_dispatch_returns_valid_task_id() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 10}))]);
    let (task_id, _rx) = engine.dispatch(flow);
    assert!(!task_id.is_empty(), "task_id must be non-empty");
}

#[tokio::test]
async fn test_dispatch_returns_unique_task_ids() {
    let engine = ThinkFlow::new();
    let flow1 = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 10}))]);
    let flow2 = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 10}))]);
    let (id1, _) = engine.dispatch(flow1);
    let (id2, _) = engine.dispatch(flow2);
    assert_ne!(id1, id2, "each dispatch must produce a unique task id");
}

// ===========================================================================
// 2. Query task progress
// ===========================================================================

#[tokio::test]
async fn test_query_progress_while_running() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 500}))]);
    let (task_id, mut rx) = engine.dispatch(flow);

    // Give the task a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let progress = engine.query_progress(&task_id);
    assert!(progress.is_some(), "progress should be available");
    let progress = progress.unwrap();
    assert!(
        progress.status == TaskStatus::Running || progress.status == TaskStatus::Done,
        "status should be Running or Done, got {:?}",
        progress.status
    );
    assert_eq!(progress.total_nodes, 1);

    let _ = rx.recv().await;
}

#[tokio::test]
async fn test_query_progress_unknown_task() {
    let engine = ThinkFlow::new();
    assert!(engine.query_progress("nonexistent-id").is_none());
}

// ===========================================================================
// 3. Receive completion notification
// ===========================================================================

#[tokio::test]
async fn test_completion_notification() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 10}))]);
    let (task_id, mut rx) = engine.dispatch(flow);

    let completion = rx.recv().await.expect("should receive completion");
    assert_eq!(completion.task_id, task_id);
    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.status, FlowStatus::Done);
}

// ===========================================================================
// 4. Non-blocking execution (dispatch returns instantly)
// ===========================================================================

#[tokio::test]
async fn test_dispatch_is_non_blocking() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 2000}))]);

    let start = std::time::Instant::now();
    let (_task_id, _rx) = engine.dispatch(flow);
    let dispatch_time = start.elapsed();

    assert!(
        dispatch_time < std::time::Duration::from_millis(100),
        "dispatch should return almost instantly, took {dispatch_time:?}"
    );
}

// ===========================================================================
// 5. Condition / if-else branching
// ===========================================================================

#[tokio::test]
async fn test_condition_true_branch() {
    let engine = ThinkFlow::new();
    let true_node = serde_json::to_value(default_node(
        NodeType::Shell,
        json!({"command": "echo", "args": ["true_branch"], "timeout": 5000}),
    ))
    .unwrap();
    let false_node = serde_json::to_value(default_node(
        NodeType::Shell,
        json!({"command": "echo", "args": ["false_branch"], "timeout": 5000}),
    ))
    .unwrap();

    let flow = simple_flow(vec![default_node(
        NodeType::Condition,
        json!({
            "mode": 0,
            "condition": "1 == 1",
            "nodes": [[true_node], [false_node]]
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.status, FlowStatus::Done);

    // Verify condition extra data
    let cond_result = &completion.result.results[0];
    assert_eq!(cond_result.status, NodeStatus::Done);
    let extra = cond_result.extra.as_ref().unwrap();
    assert_eq!(extra["result"], json!(true));
    assert_eq!(extra["branch"], json!(0));

    // Verify true branch was executed, false branch was skipped
    let children = cond_result.children.as_ref().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0][0].status, NodeStatus::Done);
    assert_eq!(children[1][0].status, NodeStatus::Skip);
}

#[tokio::test]
async fn test_condition_false_branch() {
    let engine = ThinkFlow::new();
    let true_node = serde_json::to_value(default_node(
        NodeType::Shell,
        json!({"command": "echo", "args": ["true_branch"], "timeout": 5000}),
    ))
    .unwrap();
    let false_node = serde_json::to_value(default_node(
        NodeType::Shell,
        json!({"command": "echo", "args": ["false_branch"], "timeout": 5000}),
    ))
    .unwrap();

    let flow = simple_flow(vec![default_node(
        NodeType::Condition,
        json!({
            "mode": 0,
            "condition": "1 == 2",
            "nodes": [[true_node], [false_node]]
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);

    let cond_result = &completion.result.results[0];
    let extra = cond_result.extra.as_ref().unwrap();
    assert_eq!(extra["result"], json!(false));
    assert_eq!(extra["branch"], json!(1));

    let children = cond_result.children.as_ref().unwrap();
    assert_eq!(children[0][0].status, NodeStatus::Skip);
    assert_eq!(children[1][0].status, NodeStatus::Done);
}

#[tokio::test]
async fn test_condition_with_variable() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("flag".into(), json!(true));

    let if_node = serde_json::to_value(default_node(NodeType::Sleep, json!({"time": 10}))).unwrap();
    let else_node = serde_json::to_value(default_node(NodeType::Sleep, json!({"time": 10}))).unwrap();

    let flow = flow_with_vars(
        vec![default_node(
            NodeType::Condition,
            json!({
                "mode": 0,
                "condition": "${flag} == true",
                "nodes": [[if_node], [else_node]]
            }),
        )],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();
    assert_eq!(completion.status, TaskStatus::Done);

    let extra = completion.result.results[0].extra.as_ref().unwrap();
    assert_eq!(extra["result"], json!(true));
    assert_eq!(extra["branch"], json!(0));
}

// ===========================================================================
// 6. Loop execution
// ===========================================================================

#[tokio::test]
async fn test_loop_sequential() {
    let engine = ThinkFlow::new();
    let sleep_node = serde_json::to_value(default_node(NodeType::Sleep, json!({"time": 10}))).unwrap();

    let flow = simple_flow(vec![default_node(
        NodeType::Loop,
        json!({
            "count": 3,
            "nodes": [sleep_node]
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.status, FlowStatus::Done);

    let loop_result = &completion.result.results[0];
    assert_eq!(loop_result.status, NodeStatus::Done);
    let extra = loop_result.extra.as_ref().unwrap();
    assert_eq!(extra["count"], json!(3));
    assert_eq!(extra["iterations"], json!(3));

    let children = loop_result.children.as_ref().unwrap();
    assert_eq!(children.len(), 3);
    for iter_results in children {
        assert_eq!(iter_results[0].status, NodeStatus::Done);
    }
}

#[tokio::test]
async fn test_loop_with_shell_per_iteration() {
    let engine = ThinkFlow::new();
    let shell_node = serde_json::to_value(default_node(
        NodeType::Shell,
        json!({"command": "echo", "args": ["iteration"], "timeout": 5000}),
    ))
    .unwrap();

    let flow = simple_flow(vec![default_node(
        NodeType::Loop,
        json!({
            "count": 2,
            "nodes": [shell_node]
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);

    let children = completion.result.results[0].children.as_ref().unwrap();
    assert_eq!(children.len(), 2);
    for iter_results in children {
        let shell_result = &iter_results[0];
        assert_eq!(shell_result.status, NodeStatus::Done);
        let output = shell_result.output.as_ref().unwrap();
        assert!(output["stdout"].as_str().unwrap().contains("iteration"));
    }
}

// ===========================================================================
// 7. Shell action
// ===========================================================================

#[tokio::test]
async fn test_shell_echo() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Shell,
        json!({
            "command": "echo",
            "args": ["hello from shell"],
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.status, FlowStatus::Done);

    let node_result = &completion.result.results[0];
    assert_eq!(node_result.status, NodeStatus::Done);
    let output = node_result.output.as_ref().expect("shell should have output");
    assert_eq!(output["exit_code"], json!(0));
    assert!(
        output["stdout"].as_str().unwrap().contains("hello from shell"),
        "stdout should contain our message"
    );
}

#[tokio::test]
async fn test_shell_exit_code() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Shell,
        json!({
            "command": "exit 42",
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Error);
    let node_result = &completion.result.results[0];
    assert_eq!(node_result.status, NodeStatus::Error);
    let output = node_result.output.as_ref().unwrap();
    assert_eq!(output["exit_code"], json!(42));
}

#[tokio::test]
async fn test_shell_stderr() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Shell,
        json!({
            "command": "echo err_msg >&2",
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    let output = completion.result.results[0].output.as_ref().unwrap();
    assert!(output["stderr"].as_str().unwrap().contains("err_msg"));
}

// ===========================================================================
// 8. Sleep node
// ===========================================================================

#[tokio::test]
async fn test_sleep_node() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 100}))]);
    let (_id, mut rx) = engine.dispatch(flow);

    let start = std::time::Instant::now();
    let completion = rx.recv().await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(completion.status, TaskStatus::Done);
    assert!(
        elapsed >= std::time::Duration::from_millis(80),
        "sleep should take at least ~100ms, took {elapsed:?}"
    );

    let extra = completion.result.results[0].extra.as_ref().unwrap();
    assert_eq!(extra["time"], json!(100));
}

// ===========================================================================
// 9. Script (Rhai) execution
// ===========================================================================

#[tokio::test]
async fn test_script_basic_expression() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Script,
        json!({
            "script": "let x = 40 + 2; x",
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    let output = completion.result.results[0].output.as_ref().unwrap();
    assert_eq!(*output, json!(42));
}

#[tokio::test]
async fn test_script_string_result() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Script,
        json!({
            "script": r#"let msg = "hello"; msg"#,
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    let output = completion.result.results[0].output.as_ref().unwrap();
    assert_eq!(*output, json!("hello"));
}

#[tokio::test]
async fn test_script_sys_set_get() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![
        // Script sets a variable
        default_node(
            NodeType::Script,
            json!({
                "script": r#"sys_set("my_var", 99); 0"#,
                "timeout": 5000
            }),
        ),
        // Second script reads it back
        default_node(
            NodeType::Script,
            json!({
                "script": r#"let v = sys_get("my_var"); v"#,
                "timeout": 5000
            }),
        ),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);

    // Second script should return 99
    let second_output = completion.result.results[1].output.as_ref().unwrap();
    assert_eq!(*second_output, json!(99));
}

#[tokio::test]
async fn test_script_error_handling() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(
        NodeType::Script,
        json!({
            "script": "let x = undefined_var;",
            "timeout": 5000
        }),
    )]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Error);
    assert_eq!(completion.result.results[0].status, NodeStatus::Error);
}

// ===========================================================================
// 10. Assert
// ===========================================================================

#[tokio::test]
async fn test_assert_passing() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("num".into(), json!(42));
    vars.insert("msg".into(), json!("hello world"));

    let flow = flow_with_vars(
        vec![default_node(
            NodeType::Assert,
            json!({
                "asserts": [
                    {"source": "${num}", "method": "eq", "expect": 42},
                    {"source": "${msg}", "method": "contains", "expect": "hello"},
                    {"source": "${num}", "method": "gt", "expect": 10},
                    {"source": "${msg}", "method": "type", "expect": "string"},
                    {"source": "${msg}", "method": "not_empty"},
                ]
            }),
        )],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    let output = completion.result.results[0].output.as_ref().unwrap();
    assert_eq!(output["passed"], json!(true));
}

#[tokio::test]
async fn test_assert_failing() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("num".into(), json!(10));

    let flow = flow_with_vars(
        vec![default_node(
            NodeType::Assert,
            json!({
                "asserts": [
                    {"source": "${num}", "method": "eq", "expect": 999}
                ]
            }),
        )],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Error);
    let output = completion.result.results[0].output.as_ref().unwrap();
    assert_eq!(output["passed"], json!(false));
}

#[tokio::test]
async fn test_assert_ne_and_regex() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("val".into(), json!("abc-123"));

    let flow = flow_with_vars(
        vec![default_node(
            NodeType::Assert,
            json!({
                "asserts": [
                    {"source": "${val}", "method": "ne", "expect": "xyz"},
                    {"source": "${val}", "method": "regex", "expect": "^abc-\\d+$"},
                ]
            }),
        )],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();
    assert_eq!(completion.status, TaskStatus::Done);
}

// ===========================================================================
// 11. Variable passing between nodes
// ===========================================================================

#[tokio::test]
async fn test_variable_passing_shell_to_assert() {
    let engine = ThinkFlow::new();
    // Shell sets RESPONSE_BODY; assignment extracts it; assert verifies it.
    let flow = simple_flow(vec![
        default_node(
            NodeType::Shell,
            json!({
                "command": "echo",
                "args": ["-n", "test_value_42"],
                "timeout": 5000
            }),
        ),
        default_node(
            NodeType::Assignment,
            json!({
                "assignments": [
                    {"variable": "captured", "method": "body", "expression": ""}
                ]
            }),
        ),
        default_node(
            NodeType::Assert,
            json!({
                "asserts": [
                    {"source": "${captured}", "method": "contains", "expect": "test_value_42"}
                ]
            }),
        ),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    // Assert should pass
    let assert_result = find_result_by_type(&completion.result.results, NodeType::Assert).unwrap();
    let output = assert_result.output.as_ref().unwrap();
    assert_eq!(output["passed"], json!(true));
}

#[tokio::test]
async fn test_variable_passing_script_to_script() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![
        // First script sets a variable
        default_node(
            NodeType::Script,
            json!({
                "script": r#"sys_set("counter", 100); 0"#,
                "timeout": 5000
            }),
        ),
        // Second script reads and doubles it
        default_node(
            NodeType::Script,
            json!({
                "script": r#"let c = sys_get("counter"); c * 2"#,
                "timeout": 5000
            }),
        ),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    let second_output = completion.result.results[1].output.as_ref().unwrap();
    assert_eq!(*second_output, json!(200));
}

#[tokio::test]
async fn test_flow_level_variables() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("greeting".into(), json!("howdy"));

    let flow = flow_with_vars(
        vec![default_node(
            NodeType::Assert,
            json!({
                "asserts": [
                    {"source": "${greeting}", "method": "eq", "expect": "howdy"}
                ]
            }),
        )],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();
    assert_eq!(completion.status, TaskStatus::Done);
}

// ===========================================================================
// Additional: Assignment node
// ===========================================================================

#[tokio::test]
async fn test_assignment_value_method() {
    let engine = ThinkFlow::new();
    let mut vars: Variable = HashMap::new();
    vars.insert("base".into(), json!(10));

    let flow = flow_with_vars(
        vec![
            default_node(
                NodeType::Assignment,
                json!({
                    "assignments": [
                        {"variable": "copied", "method": "value", "expression": "${base}"}
                    ]
                }),
            ),
            default_node(
                NodeType::Assert,
                json!({
                    "asserts": [
                        {"source": "${copied}", "method": "eq", "expect": 10}
                    ]
                }),
            ),
        ],
        vars,
    );
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();
    assert_eq!(completion.status, TaskStatus::Done);
}

// ===========================================================================
// Additional: Multiple parallel tasks
// ===========================================================================

#[tokio::test]
async fn test_multiple_tasks_parallel() {
    let engine = ThinkFlow::new();

    let flow1 = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 50}))]);
    let flow2 = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 50}))]);
    let flow3 = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 50}))]);

    let (id1, mut rx1) = engine.dispatch(flow1);
    let (id2, mut rx2) = engine.dispatch(flow2);
    let (id3, mut rx3) = engine.dispatch(flow3);

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    let (c1, c2, c3) = tokio::join!(rx1.recv(), rx2.recv(), rx3.recv());
    assert_eq!(c1.unwrap().status, TaskStatus::Done);
    assert_eq!(c2.unwrap().status, TaskStatus::Done);
    assert_eq!(c3.unwrap().status, TaskStatus::Done);
}

// ===========================================================================
// Additional: Error bypass (node error skips subsequent nodes)
// ===========================================================================

#[tokio::test]
async fn test_error_bypass() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![
        default_node(
            NodeType::Shell,
            json!({
                "command": "exit 1",
                "timeout": 2000
            }),
        ),
        default_node(NodeType::Sleep, json!({"time": 10})),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Error);
    assert_eq!(completion.result.results[0].status, NodeStatus::Error);
    // Second node should be skipped due to bypass
    assert_eq!(completion.result.results[1].status, NodeStatus::Skip);
}

// ===========================================================================
// Additional: list_tasks and get_result
// ===========================================================================

#[tokio::test]
async fn test_list_tasks() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 300}))]);
    let (task_id, mut rx) = engine.dispatch(flow);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let tasks = engine.list_tasks();
    assert!(!tasks.is_empty(), "should have at least one task");
    assert!(
        tasks.iter().any(|t| t.task_id == task_id),
        "our task should be in the list"
    );

    let _ = rx.recv().await;
}

#[tokio::test]
async fn test_get_result_after_completion() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 10}))]);
    let (task_id, mut rx) = engine.dispatch(flow);

    let _ = rx.recv().await;
    // Allow a tiny bit for state update
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let result = engine.get_result(&task_id);
    assert!(result.is_some(), "result should be available after completion");
    assert_eq!(result.unwrap().status, FlowStatus::Done);
}

// ===========================================================================
// Additional: Cancel a task
// ===========================================================================

#[tokio::test]
async fn test_cancel_task() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![default_node(NodeType::Sleep, json!({"time": 5000}))]);
    let (task_id, _rx) = engine.dispatch(flow);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let cancelled = engine.cancel(&task_id);
    assert!(cancelled, "should be able to cancel a running task");

    let progress = engine.query_progress(&task_id).unwrap();
    assert_eq!(progress.status, TaskStatus::Cancelled);
}

// ===========================================================================
// Additional: Disabled node is skipped
// ===========================================================================

#[tokio::test]
async fn test_disabled_node_skipped() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![
        NodeData {
            node_type: NodeType::Shell,
            id: None,
            name: Some("disabled shell".into()),
            remark: None,
            options: NodeOptions(1), // DISABLED bit
            scripts: None,
            asserts: None,
            assignments: None,
            data: json!({"command": "false", "timeout": 1000}),
        },
        default_node(NodeType::Sleep, json!({"time": 10})),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    // Disabled node is skipped, does NOT produce an error
    assert_eq!(completion.result.results[0].status, NodeStatus::Skip);
    assert_eq!(completion.result.results[1].status, NodeStatus::Done);
}

// ===========================================================================
// Additional: Multi-node sequential flow
// ===========================================================================

#[tokio::test]
async fn test_multi_node_sequential_flow() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![
        default_node(NodeType::Sleep, json!({"time": 10})),
        default_node(
            NodeType::Shell,
            json!({"command": "echo", "args": ["step2"], "timeout": 5000}),
        ),
        default_node(NodeType::Sleep, json!({"time": 10})),
    ]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.results.len(), 3);
    for r in &completion.result.results {
        assert_eq!(r.status, NodeStatus::Done);
    }
}

// ===========================================================================
// Additional: Empty flow
// ===========================================================================

#[tokio::test]
async fn test_empty_flow() {
    let engine = ThinkFlow::new();
    let flow = simple_flow(vec![]);
    let (_id, mut rx) = engine.dispatch(flow);
    let completion = rx.recv().await.unwrap();

    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.result.results.len(), 0);
}
