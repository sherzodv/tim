use std::sync::Arc;

use crate::{
    api::TimiteCapabilities,
    tim_storage::{TimStorage, TimStorageError},
};

#[derive(Debug, thiserror::Error)]
pub enum TimCapabilityError {
    #[error("Storage error")]
    StorageError(#[from] TimStorageError),
}

pub struct TimCapability {
    t_store: Arc<TimStorage>,
}

impl TimCapability {
    pub fn new(t_store: Arc<TimStorage>) -> Result<Self, TimCapabilityError> {
        Ok(Self { t_store })
    }

    pub fn list(&self) -> Result<Vec<TimiteCapabilities>, TimCapabilityError> {
        Ok(self.t_store.list_capabilities()?)
    }
}
