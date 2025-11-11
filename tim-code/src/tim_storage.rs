use crate::{
    api::{Session, Timite},
    kvstore::{KvStore, KvStoreError},
};
use prost::Message;

mod key {
    pub fn timite_prefix() -> Vec<u8> {
        b"t:id:".to_vec()
    }

    pub fn timite(id: u64) -> Vec<u8> {
        let mut k = timite_prefix();
        k.extend(id.to_be_bytes()); // big-endian = lexicographically sortable
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
        let bytes = timite.encode_to_vec();
        self.store.store_data(&key::timite(timite.id), &bytes)?;
        Ok(())
    }

    pub fn find_timite_by_id(&self, timite_id: u64) -> Result<Option<Timite>, TimStorageError> {
        Ok(self.store.fetch_data::<Timite>(&key::timite(timite_id))?)
    }

    pub fn store_session(&self, session: &Session) -> Result<(), TimStorageError> {
        let bytes = session.encode_to_vec();
        self.store
            .store_secret(&key::session(&session.key), &bytes)?;
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
