use crate::model::Config;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Configuration I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

pub struct Storage {
    path: PathBuf,
}

impl Storage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Config, StorageError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e.into()),
        };
        Self::parse(&text)
    }

    pub fn read_import(path: &Path) -> Result<Config, StorageError> {
        // Unlike startup, a missing import file must never mean an empty config.
        Self::parse(&fs::read_to_string(path)?)
    }

    pub fn parse(text: &str) -> Result<Config, StorageError> {
        let mut config: Config =
            toml::from_str(text).map_err(|_| StorageError::Invalid("Cannot parse TOML".into()))?;
        config.normalize_loaded();
        config.validate().map_err(StorageError::Invalid)?;
        Ok(config)
    }

    // Caller serializes mutations; temporary file lives on the same filesystem.
    pub fn save(&self, config: &Config) -> Result<(), StorageError> {
        config.validate().map_err(StorageError::Invalid)?;
        let text = toml::to_string_pretty(config)
            .map_err(|_| StorageError::Invalid("Cannot serialize configuration".into()))?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(text.as_bytes())?;
        temp.as_file().sync_all()?;
        if self.path.exists() {
            // A new timestamped backup is never allowed to replace an older backup.
            let mut backup = tempfile::Builder::new()
                .prefix("config-backup-")
                .suffix(".toml")
                .tempfile_in(parent)?;
            backup.write_all(&fs::read(&self.path)?)?;
            backup.as_file().sync_all()?;
            backup.keep().map_err(|e| e.error)?;
        }
        temp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_toml_roundtrip_resets_runtime_and_imports_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "version = 1\n[[jumpers]]\nid = 1\nhost = 'localhost'\nport = 22\nuser = 'test'\nauth_type = 'ssh_agent'\n[[groups]]\nid = 2\nname = 'Services'\n[[tunnels]]\nid = 3\nname = 'Test'\ngroup_id = 2\njumper_ids = [1]\nlocal_port = 8080\nremote_port = 80\nstatus = 'running'\n";
        fs::write(&path, original).unwrap();
        let store = Storage::new(path);
        let config = store.load().unwrap();
        assert_eq!(config.groups[0].tunnel_ids, vec![3]);
        assert_eq!(config.tunnels[0].status, "stopped");
        assert_eq!(config.tunnels[0].reconnect_mode, "legacy");
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap().tunnels, config.tunnels);
        let backup = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config-backup-")
            })
            .unwrap();
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), original);
    }
    #[test]
    fn malformed_input_is_not_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "not valid [ TOML").unwrap();
        assert!(Storage::new(&path).load().is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "not valid [ TOML");
    }
}
