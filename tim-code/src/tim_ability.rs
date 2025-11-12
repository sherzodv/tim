use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::api::CallAbility;
use crate::api::Session;
use crate::api::TimiteAbilities;
use crate::tim_space::TimSpace;
use crate::tim_space::TimSpaceError;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

#[derive(Debug, thiserror::Error)]
pub enum TimAbilityError {
    #[error("Storage error")]
    StorageError(#[from] TimStorageError),

    #[error("Space error")]
    SpaceError(#[from] TimSpaceError),

    #[error("Call ability {0} not found")]
    CallAbilityMissing(u64),
}

pub struct TimAbility {
    t_store: Arc<TimStorage>,
    t_space: Arc<TimSpace>,
    call_ability_cnt: AtomicU64,
}

impl TimAbility {
    pub fn new(t_store: Arc<TimStorage>, t_space: Arc<TimSpace>) -> Result<Self, TimAbilityError> {
        let max_call_id = t_store.fetch_max_call_ability_id()?;
        Ok(Self {
            t_store,
            t_space,
            call_ability_cnt: AtomicU64::new(max_call_id),
        })
    }

    pub fn list(&self) -> Result<Vec<TimiteAbilities>, TimAbilityError> {
        Ok(self.t_store.list_abilities()?)
    }

    pub async fn process_call_ability(
        &self,
        call_ability: &CallAbility,
        session: &Session,
    ) -> Result<u64, TimAbilityError> {
        let call_ability_id = self.call_ability_cnt.fetch_add(1, Ordering::Relaxed) + 1;
        self.t_store
            .store_call_ability(call_ability_id, call_ability)?;
        let mut call_ability_with_id = call_ability.clone();
        call_ability_with_id.call_ability_id = Some(call_ability_id);
        call_ability_with_id.sender_id = session.timite_id;
        self.t_space
            .publish_call_ability(&call_ability_with_id)
            .await?;
        Ok(call_ability_id)
    }

    pub fn find_call_ability(&self, call_ability_id: u64) -> Result<CallAbility, TimAbilityError> {
        self.t_store
            .fetch_call_ability(call_ability_id)?
            .ok_or(TimAbilityError::CallAbilityMissing(call_ability_id))
    }
}
