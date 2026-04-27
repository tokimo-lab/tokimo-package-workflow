use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type Variable = HashMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum VariableScope {
    Global = 0,
    Env = 1,
    Execute = 2,
    Context = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReplaceMode {
    Auto,
    #[default]
    String,
    Syntax,
}
