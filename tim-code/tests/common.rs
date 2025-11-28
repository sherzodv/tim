use std::sync::Arc;

use tempfile::tempdir;
use tempfile::TempDir;
use tim_code::tim_ability::TimAbility;
use tim_code::tim_api::TimApi;
use tim_code::tim_message::TimMessage;
use tim_code::tim_session::TimSession;
use tim_code::tim_space::TimSpace;
use tim_code::tim_storage::TimStorage;
use tim_code::tim_timite::TimTimite;

pub struct TimApiTestCtx {
    _temp_dir: TempDir,
    api: Arc<TimApi>,
}

impl TimApiTestCtx {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("kv");
        let db_path = db_path.to_string_lossy().to_string();

        let storage = Arc::new(TimStorage::new(&db_path)?);
        let session = Arc::new(TimSession::new(storage.clone()));
        let space = Arc::new(TimSpace::new(storage.clone())?);
        let timite = Arc::new(TimTimite::new(storage.clone())?);
        let ability = Arc::new(TimAbility::new(storage.clone(), space.clone())?);
        let message = Arc::new(TimMessage::new(storage.clone(), space.clone())?);
        let api = Arc::new(TimApi::new(session, space, timite, ability, message));

        Ok(Self {
            _temp_dir: temp_dir,
            api,
        })
    }

    pub fn api(&self) -> Arc<TimApi> {
        self.api.clone()
    }
}
