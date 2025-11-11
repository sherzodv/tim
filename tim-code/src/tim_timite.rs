use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::{
    api::Timite,
    tim_storage::{TimStorage, TimStorageError},
};

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
            id_cnt: Arc::new(AtomicU64::new(max_id + 1)),
        })
    }

    pub fn create(&self, nick: &str) -> Result<Timite, TimTimiteError> {
        let id = self.id_cnt.fetch_add(1, Ordering::Relaxed) + 1;
        let timite = Timite { id, nick: nick.to_string() };
        Ok(self.t_store.store_timite(&timite).map(|_| timite)?)
    }
}
