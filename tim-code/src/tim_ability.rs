use std::sync::Arc;

use crate::api::TimiteAbilities;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

#[derive(Debug, thiserror::Error)]
pub enum TimAbilityError {
    #[error("Storage error")]
    StorageError(#[from] TimStorageError),
}

pub struct TimAbility {
    t_store: Arc<TimStorage>,
}

impl TimAbility {
    pub fn new(t_store: Arc<TimStorage>) -> Result<Self, TimAbilityError> {
        Ok(Self { t_store })
    }

    pub fn list(&self) -> Result<Vec<TimiteAbilities>, TimAbilityError> {
        Ok(self.t_store.list_abilities()?)
    }
}
