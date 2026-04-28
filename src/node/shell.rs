use crate::engine::node_registry::NodeRegistry;
use crate::model::context::ExecutionContext;
use crate::model::node::*;
use crate::model::variable::Variable;
use crate::node::traits::NodeResult;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};

pub fn register(registry: &mut NodeRegistry) {
    registry.register(
        NodeType::Shell,
        Arc::new(|node, ctx, local_vars| Box::pin(execute_shell(node, ctx, local_vars))),
    );
}

async fn execute_shell(node: NodeData, ctx: ExecutionContext, local_vars: Variable) -> NodeResult {
    let node_id = node.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut result = NodeResult::new(node_id, NodeType::Shell);
    result.status = NodeStatus::Running;

    let shell_data: ShellNodeData = match serde_json::from_value(node.data.clone()) {
        Ok(d) => d,
        Err(e) => {
            result.set_error(format!("Failed to parse Shell data: {e}"), NodeError::EXECUTE_ERROR);
            return result;
        }
    };

    // Variable replacement
    let var_mgr = ctx.variable.read().await;
    let command = var_mgr.replace_to_string_with_local(&shell_data.command, &local_vars);
    let args: Vec<String> = shell_data
        .args
        .as_ref()
        .map(|a| {
            a.iter()
                .map(|arg| var_mgr.replace_to_string_with_local(arg, &local_vars))
                .collect()
        })
        .unwrap_or_default();
    let working_dir = shell_data
        .working_dir
        .as_ref()
        .map(|d| var_mgr.replace_to_string_with_local(d, &local_vars));
    let stdin_data = shell_data
        .stdin
        .as_ref()
        .map(|s| var_mgr.replace_to_string_with_local(s, &local_vars));
    let env_vars: Vec<(String, String)> = shell_data
        .env
        .as_ref()
        .map(|envs| {
            envs.iter()
                .map(|kv| {
                    (
                        var_mgr.replace_to_string_with_local(&kv.key, &local_vars),
                        var_mgr.replace_to_string_with_local(&kv.value, &local_vars),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    drop(var_mgr);

    let timeout_ms = shell_data.timeout.unwrap_or(30000);

    info!(command = %command, "Shell execution");

    // Combine command with args
    let full_command = if args.is_empty() {
        command.clone()
    } else {
        format!("{} {}", command, args.join(" "))
    };

    // Build command (platform-specific shell)
    #[cfg(windows)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&full_command);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&full_command);
        c
    };

    if let Some(dir) = &working_dir {
        cmd.current_dir(dir);
    }

    for (key, val) in &env_vars {
        cmd.env(key, val);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    if stdin_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!(command = %command, error = %e, "Failed to spawn shell command");
            result.set_error(format!("Failed to spawn command: {e}"), NodeError::SYSTEM_ERROR);
            return result;
        }
    };

    // Write stdin if provided
    if let Some(input) = &stdin_data
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(input.as_bytes()).await;
        drop(stdin);
    }

    // Wait with timeout
    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;

    match output {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            let output_data = serde_json::json!({
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr,
            });

            result.output = Some(output_data.clone());
            result.extra = Some(serde_json::json!({
                "command": full_command,
                "exit_code": exit_code,
                "working_dir": working_dir,
            }));

            // Set RESPONSE variables
            {
                let mut var_mgr = ctx.variable.write().await;
                var_mgr.set("RESPONSE_BODY", serde_json::Value::String(stdout.clone()));
                var_mgr.set("RESPONSE_DATA", output_data);
            }

            if exit_code != 0 {
                result.set_error(
                    format!("Command exited with code {}: {}", exit_code, stderr.trim()),
                    NodeError::EXECUTE_ERROR,
                );
            } else {
                result.status = NodeStatus::Done;
            }
        }
        Ok(Err(e)) => {
            error!(command = %command, error = %e, "Shell command failed");
            result.set_error(format!("Command execution failed: {e}"), NodeError::SYSTEM_ERROR);
        }
        Err(_) => {
            error!(command = %command, timeout_ms = timeout_ms, "Shell command timed out");
            result.set_error(
                format!("Command timed out after {timeout_ms}ms"),
                NodeError::EXECUTE_ERROR,
            );
        }
    }

    result
}
