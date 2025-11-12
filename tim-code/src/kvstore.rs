use std::path::Path;
use std::sync::Arc;

use prost::Message;
use rocksdb::ColumnFamily;
use rocksdb::DBAccess;
use rocksdb::DBRawIteratorWithThreadMode;
use rocksdb::Options;
use rocksdb::DB;

#[derive(Debug, thiserror::Error)]
pub enum KvStoreError {
    #[error("{0}")]
    KeysetNotFound(String),

    #[error("{0}")]
    RocksDbError(#[from] rocksdb::Error),

    #[error("Protobuf decode error: {0}")]
    DecodeError(#[from] prost::DecodeError),
}

pub struct KvStore {
    db: Arc<DB>,
}

const F_SECRETS: &str = "secrets";
const F_DATA: &str = "data";
const F_LOG: &str = "log";

const FAMILIES: &[&str] = &[F_SECRETS, F_DATA, F_LOG];

impl KvStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<KvStore, KvStoreError> {
        let db = start_rocks_db(path)?;
        Ok(KvStore { db: Arc::new(db) })
    }

    // Fetches value with max lexicographic key having given prefix `prefix`
    pub fn fetch_max_data<V: Message + Default>(
        &self,
        prefix: &[u8],
    ) -> Result<Option<V>, KvStoreError> {
        let cf = get_cf(&self.db, F_DATA)?;

        let mut iter = self.db.raw_iterator_cf(&cf);
        let bytes = collect_last_prefixed_value(&mut iter, prefix)?;

        match bytes {
            Some(data) => {
                let msg = V::decode(data.as_slice())?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    // Fetches all values with the given prefix `prefix`
    pub fn fetch_all_data<V: Message + Default>(
        &self,
        prefix: &[u8],
    ) -> Result<Vec<V>, KvStoreError> {
        let cf = get_cf(&self.db, F_DATA)?;

        let mut iter = self.db.raw_iterator_cf(&cf);
        let entries = collect_prefixed_entries(&mut iter, prefix)?;

        let mut result: Vec<V> = Vec::new();

        for bytes in entries {
            let value = V::decode(bytes.as_slice())?;
            result.push(value);
        }

        Ok(result)
    }

    pub fn fetch_secret<V: Message + Default>(
        &self,
        key: &[u8],
    ) -> Result<Option<V>, KvStoreError> {
        let cf = get_cf(&self.db, F_SECRETS)?;
        self.get_value(cf, key)
    }

    pub fn store_secret<V: Message + Default>(
        &self,
        key: &[u8],
        value: &V,
    ) -> Result<(), KvStoreError> {
        let cf = get_cf(&self.db, F_SECRETS)?;
        self.put_value(cf, key, value)
    }

    pub fn fetch_data<V: Message + Default>(&self, key: &[u8]) -> Result<Option<V>, KvStoreError> {
        let cf = get_cf(&self.db, F_DATA)?;
        self.get_value::<V>(cf, key)
    }

    pub fn store_data<V: Message + Default>(
        &self,
        key: &[u8],
        value: &V,
    ) -> Result<(), KvStoreError> {
        let cf = get_cf(&self.db, F_DATA)?;
        self.put_value(cf, key, value)
    }

    fn get_value<V: Message + Default>(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
    ) -> Result<Option<V>, KvStoreError> {
        match self.db.get_cf(cf, key)? {
            Some(bytes) => {
                let value = V::decode(&bytes[..])?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn put_value<V: Message + Default>(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
        value: &V,
    ) -> Result<(), KvStoreError> {
        let bytes = value.encode_to_vec();
        self.db.put_cf(cf, key, bytes)?;
        Ok(())
    }
}

fn get_cf<'a>(db: &'a DB, name: &'static str) -> Result<&'a ColumnFamily, KvStoreError> {
    db.cf_handle(name)
        .ok_or("failed")
        .map_err(|e| KvStoreError::KeysetNotFound(e.to_string()))
}

fn collect_last_prefixed_value<'a, D>(
    iter: &mut DBRawIteratorWithThreadMode<'a, D>,
    prefix: &[u8],
) -> Result<Option<Vec<u8>>, KvStoreError>
where
    D: DBAccess,
{
    iter.seek(prefix);

    let mut last_value = None;

    while iter.valid() {
        match iter.key() {
            Some(key) if key.starts_with(prefix) => {
                if let Some(value) = iter.value() {
                    last_value = Some(value.to_vec());
                }
            }
            _ => break,
        }

        iter.next();
    }

    iter.status()?;
    Ok(last_value)
}

fn collect_prefixed_entries<'a, D>(
    iter: &mut DBRawIteratorWithThreadMode<'a, D>,
    prefix: &[u8],
) -> Result<Vec<Vec<u8>>, KvStoreError>
where
    D: DBAccess,
{
    iter.seek(prefix);

    let mut result: Vec<Vec<u8>> = Vec::new();

    while iter.valid() {
        match iter.key() {
            Some(key) if key.starts_with(prefix) => {
                if let Some(value) = iter.value() {
                    result.push(value.to_vec());
                }
            }
            _ => break,
        }

        iter.next();
    }

    iter.status()?;
    Ok(result)
}

pub fn start_rocks_db<P: AsRef<Path>>(path: P) -> Result<DB, KvStoreError> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let db = DB::open_cf(&opts, path, FAMILIES)?;
    Ok(db)
}
