use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::MockServerMap;

#[derive(Debug)]
pub struct SsePactPlugin {
    pub mock_servers: MockServerMap,
}

impl SsePactPlugin {
    pub fn new() -> Self {
        SsePactPlugin {
            mock_servers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn host_to_bind_to(&self) -> Option<String> {
        None
    }
}

impl Default for SsePactPlugin {
    fn default() -> Self {
        Self::new()
    }
}
