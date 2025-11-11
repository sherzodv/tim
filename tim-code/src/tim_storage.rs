use crate::api::{Session, Timite};
use prost::Message;
use rocksdb::{Error as RocksError, Options, DB};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

mod key {
    pub fn timite_id(id: u64) -> Vec<u8> {
        format!("t:id:{}", id).into_bytes()
    }

    pub fn timite_nick(nick: &str) -> Vec<u8> {
        format!("t:nick:{}", nick).into_bytes()
    }

    pub fn timite_counter() -> &'static [u8] {
        b"cnt:t:id"
    }

    pub fn session(key: &str) -> Vec<u8> {
        format!("s:{}", key).into_bytes()
    }
}

#[derive(Serialize, Deserialize)]
struct TimiteData {
    id: u64,
    nick: String,
}

impl From<&Timite> for TimiteData {
    fn from(t: &Timite) -> Self {
        Self {
            id: t.id,
            nick: t.nick.clone(),
        }
    }
}

impl From<TimiteData> for Timite {
    fn from(t: TimiteData) -> Self {
        Timite {
            id: t.id,
            nick: t.nick,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TimStorageError {
    #[error("RocksDB error: {0}")]
    Rocks(#[from] RocksError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Protobuf decode error: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),

    #[error("Timite not found")]
    NotFound,

    #[error("Session not found")]
    SessionNotFound,
}

pub trait TimStorage: Send + Sync {
    fn register(&self, nick: &str) -> Result<u64, TimStorageError>;
    fn find_timite_by_id(&self, timite_id: u64) -> Result<Timite, TimStorageError>;
    fn find_timite_by_nick(&self, nick: &str) -> Result<Timite, TimStorageError>;
    fn store_session(&self, session: &Session) -> Result<(), TimStorageError>;
    fn find_session(&self, key: &str) -> Result<Session, TimStorageError>;
}

pub struct RocksDbStorage {
    db: Arc<DB>,
    id_counter: AtomicU64,
}

impl RocksDbStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TimStorageError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);

        let db = DB::open(&opts, path)?;

        // Load the current ID counter from storage
        let counter = Self::get_value::<u64>(&db, key::timite_counter())?.unwrap_or(1);

        Ok(Self {
            db: Arc::new(db),
            id_counter: AtomicU64::new(counter),
        })
    }

    fn get_value<V>(db: &DB, key: &[u8]) -> Result<Option<V>, TimStorageError>
    where
        V: for<'de> Deserialize<'de>,
    {
        match db.get(key)? {
            Some(bytes) => {
                let value = bincode::deserialize(&bytes)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn put_value<V>(db: &DB, key: &[u8], value: &V) -> Result<(), TimStorageError>
    where
        V: Serialize,
    {
        let bytes = bincode::serialize(value)?;
        db.put(key, bytes)?;
        Ok(())
    }

    fn next_id(&self) -> Result<u64, TimStorageError> {
        let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
        // Persist the new counter value
        Self::put_value(&self.db, key::timite_counter(), &(id + 1))?;
        Ok(id)
    }
}

impl TimStorage for RocksDbStorage {
    fn register(&self, nick: &str) -> Result<u64, TimStorageError> {
        // Check if nick already exists (secondary index stores just ID)
        if let Some(existing_id) = Self::get_value::<u64>(&self.db, &key::timite_nick(nick))? {
            return Ok(existing_id);
        }

        // Create new timite
        let id = self.next_id()?;
        let timite = Timite {
            id,
            nick: nick.to_string(),
        };
        let data = TimiteData::from(&timite);

        // Store full data by ID (primary key)
        Self::put_value(&self.db, &key::timite_id(id), &data)?;
        // Store only ID by nick (secondary index)
        Self::put_value(&self.db, &key::timite_nick(nick), &id)?;

        Ok(id)
    }

    fn find_timite_by_id(&self, timite_id: u64) -> Result<Timite, TimStorageError> {
        Self::get_value::<TimiteData>(&self.db, &key::timite_id(timite_id))?
            .map(|data| data.into())
            .ok_or(TimStorageError::NotFound)
    }

    fn find_timite_by_nick(&self, nick: &str) -> Result<Timite, TimStorageError> {
        // Lookup ID from secondary index
        let id = Self::get_value::<u64>(&self.db, &key::timite_nick(nick))?
            .ok_or(TimStorageError::NotFound)?;
        // Fetch full data using primary key
        self.find_timite_by_id(id)
    }

    fn store_session(&self, session: &Session) -> Result<(), TimStorageError> {
        let bytes = session.encode_to_vec();
        self.db.put(key::session(&session.key), bytes)?;
        Ok(())
    }

    fn find_session(&self, key: &str) -> Result<Session, TimStorageError> {
        match self.db.get(key::session(key))? {
            Some(bytes) => {
                let session = Session::decode(&bytes[..])?;
                Ok(session)
            }
            None => Err(TimStorageError::SessionNotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ClientInfo, Session};
    use prost_types::Timestamp;
    use tempfile::TempDir;

    #[test]
    fn test_register_and_find() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RocksDbStorage::new(temp_dir.path()).unwrap();

        // Register a new timite
        let id = storage.register("alice").unwrap();
        assert_eq!(id, 1);

        // Find by ID
        let timite = storage.find_timite_by_id(id).unwrap();
        assert_eq!(timite.id, 1);
        assert_eq!(timite.nick, "alice");

        // Find by nick
        let timite = storage.find_timite_by_nick("alice").unwrap();
        assert_eq!(timite.id, 1);
        assert_eq!(timite.nick, "alice");

        // Register same nick again should return same ID
        let id2 = storage.register("alice").unwrap();
        assert_eq!(id, id2);

        // Register different nick
        let id3 = storage.register("bob").unwrap();
        assert_eq!(id3, 2);
    }

    #[test]
    fn test_store_and_find_session() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RocksDbStorage::new(temp_dir.path()).unwrap();

        // Create a session
        let session = Session {
            key: "test-session-key-123".to_string(),
            timite_id: 42,
            created_at: Some(Timestamp {
                seconds: 1234567890,
                nanos: 123456789,
            }),
            client_info: Some(ClientInfo {
                platform: "web".to_string(),
            }),
        };

        // Store the session
        storage.store_session(&session).unwrap();

        // Find the session
        let found = storage.find_session("test-session-key-123").unwrap();
        assert_eq!(found.key, "test-session-key-123");
        assert_eq!(found.timite_id, 42);
        assert_eq!(found.created_at.as_ref().unwrap().seconds, 1234567890);
        assert_eq!(found.created_at.as_ref().unwrap().nanos, 123456789);
        assert_eq!(
            found.client_info.as_ref().unwrap().platform,
            "web".to_string()
        );
    }

    #[test]
    fn test_find_nonexistent_session() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RocksDbStorage::new(temp_dir.path()).unwrap();

        // Try to find a session that doesn't exist
        let result = storage.find_session("nonexistent-key");
        assert!(result.is_err());
        assert!(matches!(result, Err(TimStorageError::SessionNotFound)));
    }

    #[test]
    fn test_overwrite_session() {
        let temp_dir = TempDir::new().unwrap();
        let storage = RocksDbStorage::new(temp_dir.path()).unwrap();

        let key = "overwrite-test-key";

        // Store first session
        let session1 = Session {
            key: key.to_string(),
            timite_id: 1,
            created_at: Some(Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            client_info: Some(ClientInfo {
                platform: "mobile".to_string(),
            }),
        };
        storage.store_session(&session1).unwrap();

        // Store second session with same key
        let session2 = Session {
            key: key.to_string(),
            timite_id: 2,
            created_at: Some(Timestamp {
                seconds: 2000,
                nanos: 0,
            }),
            client_info: Some(ClientInfo {
                platform: "desktop".to_string(),
            }),
        };
        storage.store_session(&session2).unwrap();

        // Find should return the latest session
        let found = storage.find_session(key).unwrap();
        assert_eq!(found.timite_id, 2);
        assert_eq!(found.created_at.as_ref().unwrap().seconds, 2000);
        assert_eq!(
            found.client_info.as_ref().unwrap().platform,
            "desktop".to_string()
        );
    }
}
