use crate::model::variable::{ReplaceMode, Variable};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static VAR_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").unwrap());

/// Token from lexing a variable reference
#[derive(Debug, Clone)]
pub struct VarToken {
    pub full: String,
    pub key: String,
    pub path: Option<String>,
}

pub fn tokenize(input: &str) -> Vec<VarToken> {
    VAR_PATTERN
        .captures_iter(input)
        .map(|cap| {
            let full = cap[0].to_string();
            let inner = cap[1].to_string();
            let (key, path) = if let Some(dot_pos) = inner.find('.') {
                (inner[..dot_pos].to_string(), Some(inner[dot_pos + 1..].to_string()))
            } else {
                (inner, None)
            };
            VarToken { full, key, path }
        })
        .collect()
}

/// Resolve a value from a variable map by key and optional path
pub fn resolve_value(variables: &[&Variable], token: &VarToken) -> Option<Value> {
    for vars in variables {
        if let Some(value) = vars.get(&token.key) {
            if let Some(path) = &token.path {
                let obj = match value {
                    Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(value.clone()),
                    other => other.clone(),
                };
                return json_path_get(&obj, path);
            }
            return Some(value.clone());
        }
    }
    None
}

fn json_path_get(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for part in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current.clone())
}

/// Replace variables in a string
pub fn replace(input: &str, variables: &[&Variable], mode: ReplaceMode) -> Value {
    let tokens = tokenize(input);

    if tokens.is_empty() {
        return Value::String(input.to_string());
    }

    // Single variable that spans the entire string -> return native type
    if tokens.len() == 1 && input == tokens[0].full {
        if let Some(val) = resolve_value(variables, &tokens[0]) {
            return match mode {
                ReplaceMode::Auto => val,
                ReplaceMode::String => Value::String(value_to_string(&val)),
                ReplaceMode::Syntax => {
                    if val.is_string() {
                        Value::String(format!("\"{}\"", val.as_str().unwrap()))
                    } else {
                        Value::String(value_to_string(&val))
                    }
                }
            };
        }
        return Value::String(input.to_string());
    }

    // Multiple variables or mixed content -> string replacement
    let mut result = input.to_string();
    for token in &tokens {
        if let Some(val) = resolve_value(variables, token) {
            let replacement = match mode {
                ReplaceMode::Syntax if val.is_string() => {
                    format!("\"{}\"", val.as_str().unwrap())
                }
                _ => value_to_string(&val),
            };
            result = result.replace(&token.full, &replacement);
        }
    }

    Value::String(result)
}

pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Replace and return as string
pub fn replace_to_string(input: &str, variables: &[&Variable]) -> String {
    match replace(input, variables, ReplaceMode::String) {
        Value::String(s) => s,
        other => value_to_string(&other),
    }
}
