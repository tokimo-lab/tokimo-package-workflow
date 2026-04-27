use crate::model::flow::FlowDefinition;
use async_trait::async_trait;

pub mod timer;

/// Trigger trait - extensible for various trigger types
#[async_trait]
pub trait Trigger: Send + Sync {
    /// Start the trigger
    async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Stop the trigger
    async fn stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Get trigger name
    fn name(&self) -> &str;
}

/// Trigger event
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub trigger_name: String,
    pub flow: FlowDefinition,
    pub fired_at: chrono::DateTime<chrono::Utc>,
}
