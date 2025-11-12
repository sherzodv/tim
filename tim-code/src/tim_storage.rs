use crate::api::Ability;
use crate::api::CallAbility;
use crate::api::Session;
use crate::api::Timite;
use crate::api::TimiteAbilities;
use crate::kvstore::KvStore;
use crate::kvstore::KvStoreError;
use crate::storage::StoredTimiteAbilities;

mod key {
    pub fn timite_prefix() -> Vec<u8> {
        b"t:id:".to_vec()
    }

    pub fn timite_abilities_prefix() -> Vec<u8> {
        b"t:skill:".to_vec()
    }

    pub fn timite(id: u64) -> Vec<u8> {
        let mut k = timite_prefix();
        k.extend(id.to_be_bytes()); // big-endian = lexicographically sortable
        k
    }

    pub fn timite_abilities(id: u64) -> Vec<u8> {
        let mut k = timite_abilities_prefix();
        k.extend(id.to_be_bytes());
        k
    }

    pub fn session(key: &str) -> Vec<u8> {
        format!("s:{}", key).into_bytes()
    }

    pub fn ability_call_prefix() -> Vec<u8> {
        b"acall:".to_vec()
    }

    pub fn ability_call(id: u64) -> Vec<u8> {
        let mut k = ability_call_prefix();
        k.extend(id.to_be_bytes());
        k
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TimStorageError {
    #[error("Store error: {0}")]
    KvStore(#[from] KvStoreError),
}

pub struct TimStorage {
    store: KvStore,
}

impl TimStorage {
    pub fn new(path: &str) -> Result<TimStorage, TimStorageError> {
        let store = KvStore::new(path)?;
        Ok(Self { store: store })
    }

    pub fn store_timite(&self, timite: &Timite) -> Result<(), TimStorageError> {
        self.store.store_data(&key::timite(timite.id), timite)?;
        Ok(())
    }

    pub fn store_timite_abilities(
        &self,
        timite_id: u64,
        abilities: &Vec<Ability>,
    ) -> Result<(), TimStorageError> {
        let record = StoredTimiteAbilities {
            timite_id,
            abilities: abilities.to_vec(),
        };
        self.store
            .store_data(&key::timite_abilities(timite_id), &record)?;
        Ok(())
    }

    pub fn list_abilities(&self) -> Result<Vec<TimiteAbilities>, TimStorageError> {
        let list = self
            .store
            .fetch_all_data::<StoredTimiteAbilities>(&key::timite_abilities_prefix())?;

        let mut result = Vec::new();

        for tc in list {
            if let Some(timite) = self.fetch_timite(tc.timite_id)? {
                result.push(TimiteAbilities {
                    timite: Some(timite),
                    abilities: tc.abilities,
                });
            }
        }

        Ok(result)
    }

    pub fn store_session(&self, session: &Session) -> Result<(), TimStorageError> {
        self.store
            .store_secret(&key::session(&session.key), session)?;
        Ok(())
    }

    pub fn find_session(&self, key: &str) -> Result<Option<Session>, TimStorageError> {
        Ok(self.store.fetch_secret::<Session>(&key::session(key))?)
    }

    pub fn fetch_max_timite_id(&self) -> Result<u64, TimStorageError> {
        let timite_opt = self.store.fetch_max_data::<Timite>(&key::timite_prefix())?;
        Ok(if let Some(timite) = timite_opt {
            timite.id
        } else {
            0
        })
    }

    pub fn fetch_timite(&self, timite_id: u64) -> Result<Option<Timite>, TimStorageError> {
        Ok(self.store.fetch_data::<Timite>(&key::timite(timite_id))?)
    }

    pub fn fetch_max_call_ability_id(&self) -> Result<u64, TimStorageError> {
        let record = self
            .store
            .fetch_max_log::<CallAbility>(&key::ability_call_prefix())?;
        Ok(record
            .map(|entry| entry.call_ability_id.unwrap_or(0))
            .unwrap_or(0))
    }

    pub fn store_call_ability(
        &self,
        call_ability_id: u64,
        call_ability: &CallAbility,
    ) -> Result<(), TimStorageError> {
        let mut rec = call_ability.clone();
        rec.call_ability_id = Some(call_ability_id);
        self.store
            .store_log(&key::ability_call(call_ability_id), &rec)?;
        Ok(())
    }

    pub fn fetch_call_ability(
        &self,
        call_ability_id: u64,
    ) -> Result<Option<CallAbility>, TimStorageError> {
        let record = self
            .store
            .fetch_log::<CallAbility>(&key::ability_call(call_ability_id))?;
        Ok(record)
    }

    pub fn fetch_timite_abilities(&self, timite_id: u64) -> Result<Vec<Ability>, TimStorageError> {
        let record = self
            .store
            .fetch_data::<StoredTimiteAbilities>(&key::timite_abilities(timite_id))?;
        Ok(record.map(|entry| entry.abilities).unwrap_or_default())
    }
}
