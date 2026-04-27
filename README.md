# tokimo-package-workflow

A programmable workflow engine with Rhai scripting, cron scheduling, JSONPath filtering, and extensible node-based execution.

## Features

- **Node-based execution** — compose flows from reusable node types (Shell, HTTP, Script, Condition, Loop, etc.)
- **Rhai scripting** — embed custom logic with the Rhai scripting engine
- **Cron scheduling** — trigger flows on cron schedules
- **JSONPath filtering** — filter and transform data between nodes
- **Async execution** — dispatch flows for non-blocking, parallel execution with progress tracking
- **Extensible** — register custom node types via the `NodeRegistry`

## Built-in node types

| Node | Description |
|------|-------------|
| `Shell` | Execute shell commands |
| `Http` | Make HTTP requests |
| `Script` | Run Rhai scripts |
| `Condition` | If-else branching |
| `Loop` | Loop execution |
| `DataLoop` | Iterate over data |
| `Assert` | Assert conditions on variables |
| `Assignment` | Assign and transform variables |
| `Sleep` | Pause execution |
| `Poll` | Poll external services |
| `MySQL` / `PostgreSQL` / `MongoDB` / `MSSQL` | Database queries |
| `Redis` | Redis operations |
| `gRPC` | gRPC calls |
| `TCP` / `UDP` | Raw socket communication |

## Usage

```rust
use tokimo_package_workflow::ThinkFlow;
use tokimo_package_workflow::model::flow::FlowDefinition;
use tokimo_package_workflow::model::node::{NodeData, NodeType};
use serde_json::json;

let engine = ThinkFlow::new();

let flow = FlowDefinition {
    name: Some("Hello World".into()),
    id: None,
    r#async: false,
    timeout: Some(5000),
    nodes: vec![
        NodeData {
            node_type: NodeType::Shell,
            id: None,
            name: None,
            remark: None,
            options: Default::default(),
            scripts: None,
            asserts: None,
            assignments: None,
            data: json!({"command": "echo", "args": ["hello"], "timeout": 5000}),
        }
    ],
    variables: None,
    options: None,
};

let (task_id, mut rx) = engine.dispatch(flow);
let completion = rx.recv().await.unwrap();
assert_eq!(completion.status, TaskStatus::Done);
```

## Cargo

```toml
tokimo-package-workflow = { git = "https://github.com/tokimo-lab/tokimo-package-workflow" }
```

## License

MIT
