use std::sync::Arc;
use std::sync::Mutex;

use crate::api::Ability;
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
    next_id: Mutex<u64>,
}

impl TimTimite {
    pub fn new(t_store: Arc<TimStorage>) -> Result<Self, TimTimiteError> {
        let max_id = t_store.fetch_max_timite_id()?;
        Ok(Self {
            t_store,
            next_id: Mutex::new(max_id + 1),
        })
    }

    /// The timite known by `username`, created on first use.
    pub fn connect(&self, username: &str) -> Result<Timite, TimTimiteError> {
        let mut next_id = self.next_id.lock().expect("timite id lock poisoned");
        if let Some(timite) = self.t_store.fetch_timite_by_username(username)? {
            return Ok(timite);
        }
        let timite = Timite {
            id: *next_id,
            username: username.to_string(),
        };
        self.t_store.store_timite(&timite)?;
        *next_id += 1;
        Ok(timite)
    }

    pub fn declare_abilities(
        &self,
        timite_id: u64,
        abilities: &Vec<Ability>,
    ) -> Result<(), TimTimiteError> {
        Ok(self.t_store.store_timite_abilities(timite_id, abilities)?)
    }

    pub fn get(&self, timite_id: u64) -> Result<Option<Timite>, TimTimiteError> {
        Ok(self.t_store.fetch_timite(timite_id)?)
    }
}
