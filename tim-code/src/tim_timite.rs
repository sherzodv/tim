use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::api::Capability;
use crate::api::Timite;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

#[derive(Debug, thiserror::Error)]
pub enum TimTimiteError {
    #[error("Storage error")]
    StorageError(#[from] TimStorageError),
}

pub struct TimTimite {
    t_store: Arc<TimStorage>,
    id_cnt: Arc<AtomicU64>,
}

impl TimTimite {
    pub fn new(t_store: Arc<TimStorage>) -> Result<Self, TimTimiteError> {
        let max_id = t_store.fetch_max_timite_id()?;
        Ok(Self {
            t_store,
            id_cnt: Arc::new(AtomicU64::new(max_id)),
        })
    }

    pub fn create(&self, nick: &str) -> Result<Timite, TimTimiteError> {
        let id = self.id_cnt.fetch_add(1, Ordering::Relaxed) + 1;
        let timite = Timite {
            id,
            nick: nick.to_string(),
        };
        Ok(self.t_store.store_timite(&timite).map(|_| timite)?)
    }

    pub fn declare_capabilities(
        &self,
        timite_id: u64,
        capabilities: &Vec<Capability>,
    ) -> Result<(), TimTimiteError> {
        for c in capabilities {
            self.t_store.store_timite_capability(timite_id, c)?;
        }
        Ok(())
    }
}
