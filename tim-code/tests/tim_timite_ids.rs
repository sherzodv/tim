use std::sync::Arc;

use tempfile::tempdir;
use tim_code::tim_storage::TimStorage;
use tim_code::tim_timite::TimTimite;

#[test]
fn timite_ids_survive_restart() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().join("kv");
    let db_path = db_path.to_string_lossy().to_string();

    let last_id = {
        let storage = Arc::new(TimStorage::new(&db_path)?);
        let timite = TimTimite::new(storage)?;

        let first = timite.create("alpha")?;
        let second = timite.create("beta")?;

        assert!(
            second.id > first.id,
            "IDs should increase across sequential creations"
        );

        second.id
    };

    {
        let storage = Arc::new(TimStorage::new(&db_path)?);
        let timite = TimTimite::new(storage)?;

        let after_restart = timite.create("gamma")?;
        assert_eq!(
            after_restart.id,
            last_id + 1,
            "IDs should continue from the last persisted value"
        );

        let next = timite.create("delta")?;
        assert_eq!(
            next.id,
            after_restart.id + 1,
            "IDs must keep incrementing during the same run"
        );
    }

    Ok(())
}
