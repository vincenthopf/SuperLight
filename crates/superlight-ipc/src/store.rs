use crate::Paths;
use serde_json::Value;
use std::{fs::{self, File, OpenOptions, TryLockError}, io::{self, Read, Write}, path::Path};
use superlight_core::{CONFIG_LIMIT, actions::Platform, config, policy};

pub struct Store {
    pub paths: Paths,
    pub value: Value,
    pub revision: u64,
    pub notice: Option<String>,
    pub first_run: bool,
    _lock: File,
}

impl Store {
    pub fn open(paths: Paths) -> io::Result<Self> {
        paths.prepare()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(paths.runtime.join("service.lock"))?;
        lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => io::Error::new(io::ErrorKind::AlreadyExists, "SuperLight is already running"),
            TryLockError::Error(error) => error,
        })?;
        let mut first_run = false;
        let mut notice = None;
        let value = match read_limited(&paths.config, CONFIG_LIMIT) {
            Ok(bytes) => config::parse(&bytes).map_err(invalid)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                first_run = true;
                if let Some(legacy) = paths.legacy.as_ref().filter(|path| path.is_file()) {
                    let bytes = read_limited(legacy, CONFIG_LIMIT)?;
                    let value = config::parse(&bytes).map_err(|error| invalid(format!("Existing Mouser configuration is invalid and was left unchanged: {error}")))?;
                    notice = Some("Imported Mouser settings into SuperLight. The original file and its login settings were left unchanged. Quit Mouser before using SuperLight.".into());
                    value
                } else { config::defaults() }
            }
            Err(error) => return Err(error),
        };
        policy::validate_actions(&value, Platform::current()).map_err(invalid)?;
        let mut store = Self { paths, value, revision: 1, notice, first_run, _lock: lock };
        if first_run { store.persist_current()?; }
        Ok(store)
    }

    fn persist_current(&mut self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.value).map_err(invalid)?;
        if bytes.len() > CONFIG_LIMIT { return Err(invalid("Configuration exceeds 1 MiB")); }
        let durable = atomic_write(&self.paths.config, &bytes)?;
        if !durable { self.notice = Some("Settings were saved atomically, but the filesystem could not synchronize the containing directory.".into()); }
        Ok(())
    }

    pub fn apply(&mut self, expected_revision: u64, value: Value) -> io::Result<()> {
        if expected_revision != self.revision { return Err(io::Error::new(io::ErrorKind::WouldBlock, "Settings changed in another operation. Reload before saving.")); }
        let value = config::migrate(value).map_err(invalid)?;
        policy::validate_actions(&value, Platform::current()).map_err(invalid)?;
        let bytes = serde_json::to_vec_pretty(&value).map_err(invalid)?;
        if bytes.len() > CONFIG_LIMIT { return Err(invalid("Configuration exceeds 1 MiB")); }
        let durable = atomic_write(&self.paths.config, &bytes)?;
        self.value = value;
        self.revision = self.revision.checked_add(1).ok_or_else(|| io::Error::other("Configuration revision exhausted"))?;
        if !durable { self.notice = Some("Settings were saved atomically, but the filesystem could not synchronize the containing directory.".into()); }
        Ok(())
    }
}

pub fn read_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(invalid("File exceeds the configured size limit")); }
    Ok(bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_file() || metadata.file_type().is_symlink()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Refusing to replace a symbolic link or non-file"));
    }
    let parent = path.parent().ok_or_else(|| invalid("Missing parent directory"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    let persisted = temporary.persist(path).map_err(|error| error.error)?;
    persisted.sync_all()?;
    #[cfg(unix)] { Ok(File::open(parent).and_then(|directory| directory.sync_all()).is_ok()) }
    #[cfg(not(unix))] { Ok(true) }
}

fn invalid(error: impl ToString) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, error.to_string()) }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn single_instance_lock_is_released_by_drop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(dir.path());
        let store = Store::open(paths.clone()).unwrap();
        assert_eq!(Store::open(paths.clone()).err().unwrap().kind(), io::ErrorKind::AlreadyExists);
        drop(store);
        assert!(Store::open(paths).is_ok());
    }

    #[test]
    fn configuration_changes_are_atomic_and_revision_checked() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(Paths::in_dir(dir.path())).unwrap();
        let original = fs::read(&store.paths.config).unwrap();
        let mut value = store.value.clone();
        value["settings"]["dpi"] = json!(1600);
        assert!(store.apply(0, value.clone()).is_err());
        assert_eq!(fs::read(&store.paths.config).unwrap(), original);
        store.apply(1, value).unwrap();
        assert_eq!(store.revision, 2);
        assert_eq!(config::parse(&fs::read(&store.paths.config).unwrap()).unwrap()["settings"]["dpi"], 1600);
    }

    #[test]
    fn malformed_existing_configuration_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(dir.path());
        fs::write(&paths.config, b"{broken").unwrap();
        assert!(Store::open(paths.clone()).is_err());
        assert_eq!(fs::read(paths.config).unwrap(), b"{broken");
    }

    #[test]
    fn importing_legacy_configuration_does_not_modify_its_source() {
        let root = tempfile::tempdir().unwrap();
        let mut paths = Paths::in_dir(root.path().join("new"));
        let legacy = root.path().join("legacy.json");
        let original = br#"{"version":4,"settings":{"start_with_windows":true}}"#;
        fs::write(&legacy, original).unwrap();
        paths.legacy = Some(legacy.clone());
        let store = Store::open(paths).unwrap();
        assert_eq!(store.value["settings"]["start_at_login"], true);
        assert_eq!(fs::read(legacy).unwrap(), original);
        assert!(store.notice.is_some());
    }

    #[test]
    fn oversized_files_are_rejected_before_unbounded_allocation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large");
        fs::write(&path, [b' '; 100]).unwrap();
        assert!(read_limited(&path, 99).is_err());
        assert_eq!(read_limited(&path, 100).unwrap().len(), 100);
    }

    #[test]
    fn failed_persistence_does_not_publish_new_configuration() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(Paths::in_dir(root.path())).unwrap();
        let original = store.value.clone();
        store.paths.config = root.path().join("missing").join("config.json");
        let mut value = original.clone();
        value["settings"]["dpi"] = json!(2400);
        assert!(store.apply(1, value).is_err());
        assert_eq!(store.value, original);
        assert_eq!(store.revision, 1);
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_symlink_targets_are_not_replaced() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("real");
        atomic_write(&path, b"original").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(atomic_write(&link, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }
}
