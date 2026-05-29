use std::collections::HashMap;
use std::sync::Arc;

use super::Backend;

#[derive(Default, Clone)]
pub struct BackendRegistry {
    by_name: HashMap<String, Arc<dyn Backend>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, backend: Arc<dyn Backend>) {
        self.by_name.insert(backend.name().to_string(), backend);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Backend>> {
        self.by_name.get(name).cloned()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }
}
