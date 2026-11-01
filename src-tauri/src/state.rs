// State management for session if needed
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub active_llm_cancels: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_llm_cancels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
