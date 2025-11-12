use crate::api::Capability;
use crate::api::Session;
use crate::api::Timite;
use crate::api::TimiteCapabilities;
use crate::kvstore::KvStore;
use crate::kvstore::KvStoreError;

mod key {
    pub fn timite_prefix() -> Vec<u8> {
        b"t:id:".to_vec()
    }

    pub fn timite_skill_prefix() -> Vec<u8> {
        b"t:skill:".to_vec()
    }

    pub fn timite(id: u64) -> Vec<u8> {
        let mut k = timite_prefix();
        k.extend(id.to_be_bytes()); // big-endian = lexicographically sortable
        k
    }

    pub fn timite_skill(id: u64, name: &str) -> Vec<u8> {
        let mut k = timite_skill_prefix();
        k.extend(id.to_be_bytes());
        k.extend(name.as_bytes());
        k
    }

    pub fn session(key: &str) -> Vec<u8> {
        format!("s:{}", key).into_bytes()
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

    pub fn store_timite_capability(
        &self,
        timite_id: u64,
        capability: &Capability,
    ) -> Result<(), TimStorageError> {
        self.store
            .store_data(&key::timite_skill(timite_id, &capability.name), capability)?;
        Ok(())
    }

    pub fn list_capabilities(&self) -> Result<Vec<TimiteCapabilities>, TimStorageError> {
        let res = self
            .store
            .fetch_all_data::<TimiteCapabilities>(&key::timite_skill_prefix())?;
        Ok(res)
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
}
