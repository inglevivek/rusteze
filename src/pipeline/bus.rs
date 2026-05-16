use tokio::sync::broadcast;
use super::events::PipelineEvent;

pub struct PipelineEventBus {
    tx: broadcast::Sender<PipelineEvent>,
}

impl PipelineEventBus {
    /// Initializes a bounded event bus. 
    /// `capacity` limits the number of retained messages for lagging subscribers.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emits a pipeline event to all active subscribers.
    /// Fails silently if no subscribers are currently listening.
    pub fn emit(&self, event: PipelineEvent) {
        // We ignore the error because `SendError` just means there are 0 receivers.
        // The persistence sink will be an active receiver in production.
        let _ = self.tx.send(event);
    }

    /// Creates a new subscription to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<PipelineEvent> {
        self.tx.subscribe()
    }
}
