use crate::model::flow::FlowDefinition;
use crate::trigger::{Trigger, TriggerEvent};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

pub struct TimerTrigger {
    name: String,
    cron_expr: String,
    flow: FlowDefinition,
    event_tx: mpsc::Sender<TriggerEvent>,
    scheduler: Arc<Mutex<Option<JobScheduler>>>,
}

impl TimerTrigger {
    pub fn new(
        name: impl Into<String>,
        cron_expr: impl Into<String>,
        flow: FlowDefinition,
        event_tx: mpsc::Sender<TriggerEvent>,
    ) -> Self {
        Self {
            name: name.into(),
            cron_expr: cron_expr.into(),
            flow,
            event_tx,
            scheduler: Arc::new(Mutex::new(None)),
        }
    }

    pub fn event_sender(&self) -> mpsc::Sender<TriggerEvent> {
        self.event_tx.clone()
    }
}

#[async_trait]
impl Trigger for TimerTrigger {
    async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let sched = JobScheduler::new().await?;

        let flow = self.flow.clone();
        let trigger_name = self.name.clone();
        let tx = self.event_tx.clone();

        let job = Job::new_async(self.cron_expr.as_str(), move |_uuid, _lock| {
            let flow = flow.clone();
            let trigger_name = trigger_name.clone();
            let tx = tx.clone();
            Box::pin(async move {
                let event = TriggerEvent {
                    trigger_name: trigger_name.clone(),
                    flow,
                    fired_at: chrono::Utc::now(),
                };
                if let Err(e) = tx.send(event).await {
                    error!(trigger = %trigger_name, "Failed to send trigger event: {}", e);
                } else {
                    info!(trigger = %trigger_name, "Timer trigger fired");
                }
            })
        })?;

        sched.add(job).await?;
        sched.start().await?;

        info!(name = %self.name, cron = %self.cron_expr, "Timer trigger started");

        let mut guard = self.scheduler.lock().await;
        *guard = Some(sched);

        Ok(())
    }

    async fn stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut guard = self.scheduler.lock().await;
        if let Some(mut sched) = guard.take() {
            sched.shutdown().await?;
            info!(name = %self.name, "Timer trigger stopped");
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}
