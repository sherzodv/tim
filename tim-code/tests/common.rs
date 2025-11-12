use std::sync::Arc;

use tempfile::{tempdir, TempDir};
use tim_code::tim_api::TimApi;
use tim_code::tim_capability::TimCapability;
use tim_code::tim_session::TimSession;
use tim_code::tim_space::TimSpace;
use tim_code::tim_storage::TimStorage;
use tim_code::tim_timite::TimTimite;

pub struct TimApiTestCtx {
    _temp_dir: TempDir,
    api: TimApi,
}

impl TimApiTestCtx {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("kv");
        let db_path = db_path.to_string_lossy().to_string();

        let storage = Arc::new(TimStorage::new(&db_path)?);
        let api = TimApi::new(
            Arc::new(TimSession::new(storage.clone())),
            Arc::new(TimSpace::new()),
            Arc::new(TimTimite::new(storage.clone())?),
            Arc::new(TimCapability::new(storage.clone())?),
        );

        Ok(Self {
            _temp_dir: temp_dir,
            api,
        })
    }

    pub fn api(&self) -> &TimApi {
        &self.api
    }
}
