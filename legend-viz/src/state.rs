use legend::memory::trace::TraceEvent;
use legend::memory::MemoryState;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// Shared application state for the viz server.
#[derive(Clone)]
pub struct AppState {
    pub memory: Arc<Mutex<MemoryState>>,
    pub trace_tx: broadcast::Sender<TraceEvent>,
}

impl AppState {
    pub fn new(memory: MemoryState, trace_tx: broadcast::Sender<TraceEvent>) -> Self {
        Self {
            memory: Arc::new(Mutex::new(memory)),
            trace_tx,
        }
    }
}
