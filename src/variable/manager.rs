use crate::model::variable::{ReplaceMode, Variable, VariableScope};
use crate::variable::replacer;
use serde_json::Value;
use std::collections::HashMap;

/// Priority order: Context > Execute > Env > Global
const SCOPE_PRIORITY: &[VariableScope] = &[
    VariableScope::Context,
    VariableScope::Execute,
    VariableScope::Env,
    VariableScope::Global,
];

#[derive(Debug, Clone)]
pub struct VariableManager {
    data: HashMap<VariableScope, Variable>,
}

impl VariableManager {
    pub fn new() -> Self {
        let mut data = HashMap::new();
        data.insert(VariableScope::Global, Variable::new());
        data.insert(VariableScope::Env, Variable::new());
        data.insert(VariableScope::Execute, Variable::new());
        data.insert(VariableScope::Context, Variable::new());
        Self { data }
    }

    pub fn with_variables(env: Variable, execute: Variable) -> Self {
        let mut mgr = Self::new();
        mgr.data.insert(VariableScope::Env, env);
        mgr.data.insert(VariableScope::Execute, execute);
        mgr
    }

    /// Get variable by key, searching through scopes by priority
    pub fn get(&self, key: &str) -> Option<Value> {
        self.get_with_local(key, None)
    }

    /// Get variable with an optional local/isolate scope searched first
    pub fn get_with_local(&self, key: &str, local: Option<&Variable>) -> Option<Value> {
        let token = replacer::VarToken {
            full: format!("${{{key}}}"),
            key: if let Some(dot) = key.find('.') {
                key[..dot].to_string()
            } else {
                key.to_string()
            },
            path: key.find('.').map(|dot| key[dot + 1..].to_string()),
        };

        // Search local scope first
        if let Some(local_vars) = local
            && let Some(val) = replacer::resolve_value(&[local_vars], &token)
        {
            return Some(val);
        }

        // Search scopes by priority
        for scope in SCOPE_PRIORITY {
            if let Some(vars) = self.data.get(scope)
                && let Some(val) = replacer::resolve_value(&[vars], &token)
            {
                return Some(val);
            }
        }
        None
    }

    /// Set variable in context scope
    pub fn set(&mut self, key: &str, value: Value) {
        self.set_scope(VariableScope::Context, key, value);
    }

    /// Set variable in a specific scope
    pub fn set_scope(&mut self, scope: VariableScope, key: &str, value: Value) {
        self.data.entry(scope).or_default().insert(key.to_string(), value);
    }

    /// Delete variable from context scope
    pub fn del(&mut self, key: &str) {
        if let Some(vars) = self.data.get_mut(&VariableScope::Context) {
            vars.remove(key);
        }
    }

    /// Get all variables for a scope (all scopes are always initialized)
    pub fn get_scope(&self, scope: VariableScope) -> &Variable {
        self.data.get(&scope).unwrap()
    }

    /// Get variables for replacement (ordered by priority)
    fn get_ordered_variables<'a>(&'a self, local: Option<&'a Variable>) -> Vec<&'a Variable> {
        let mut result = Vec::new();
        if let Some(local_vars) = local {
            result.push(local_vars);
        }
        for scope in SCOPE_PRIORITY {
            if let Some(vars) = self.data.get(scope) {
                result.push(vars);
            }
        }
        result
    }

    /// Replace variables in a string
    pub fn replace(&self, content: &str, mode: ReplaceMode) -> Value {
        let vars = self.get_ordered_variables(None);
        replacer::replace(content, &vars, mode)
    }

    /// Replace with local scope included
    pub fn replace_with_local(&self, content: &str, mode: ReplaceMode, local: &Variable) -> Value {
        let vars = self.get_ordered_variables(Some(local));
        replacer::replace(content, &vars, mode)
    }

    /// Replace and return as string
    pub fn replace_to_string(&self, content: &str) -> String {
        let vars = self.get_ordered_variables(None);
        replacer::replace_to_string(content, &vars)
    }

    /// Replace with local and return as string
    pub fn replace_to_string_with_local(&self, content: &str, local: &Variable) -> String {
        let vars = self.get_ordered_variables(Some(local));
        replacer::replace_to_string(content, &vars)
    }

    /// Create a child scope (inherits all parent variables)
    #[must_use]
    pub fn create_child(&self) -> Self {
        let mut child = Self::new();
        for scope in SCOPE_PRIORITY {
            if let Some(vars) = self.data.get(scope) {
                child.data.insert(*scope, vars.clone());
            }
        }
        child
    }

    /// Merge local variables into context scope
    pub fn merge_local(&mut self, local: &Variable) {
        let ctx = self.data.entry(VariableScope::Context).or_default();
        for (k, v) in local {
            ctx.insert(k.clone(), v.clone());
        }
    }
}

impl Default for VariableManager {
    fn default() -> Self {
        Self::new()
    }
}
