use crate::{
    model::Config,
    storage::{Storage, StorageError},
};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize)]
pub struct Location {
    pub implicit: PathBuf,
    pub effective: PathBuf,
    pub pointer: PathBuf,
}

#[derive(Serialize)]
pub struct Conflict {
    pub config_exists: bool,
    pub locale_exists: bool,
}

pub fn resolve(implicit: &Path) -> Result<Location, StorageError> {
    absolute(implicit)?;
    let pointer = implicit.with_file_name("config.root");
    let effective = match fs::read_to_string(&pointer) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => implicit.to_owned(),
        Err(error) => return Err(error.into()),
        Ok(text) => {
            let directory = text.lines().next().unwrap_or("").trim();
            if directory.is_empty() {
                implicit.to_owned()
            } else {
                let directory = Path::new(directory);
                absolute(directory)?;
                if !directory.is_dir() {
                    return Err(invalid(
                        "Configuration pointer target is not an available directory",
                    ));
                }
                let path = directory.join("config.toml");
                // An unavailable custom config is not permission to create an empty replacement.
                Storage::read_import(&path)?;
                path
            }
        }
    };
    Ok(Location {
        implicit: implicit.to_owned(),
        effective,
        pointer,
    })
}

pub fn conflicts(location: &Location, target: &Path) -> Result<Conflict, StorageError> {
    absolute(target)?;
    if target.exists() && !target.is_dir() {
        return Err(invalid("Configuration target is not a directory"));
    }
    let anchor = location
        .implicit
        .parent()
        .ok_or_else(|| invalid("Missing configuration anchor"))?;
    let config = if same_file(target, anchor) {
        location.implicit.clone()
    } else {
        target.join("config.toml")
    };
    Ok(Conflict {
        config_exists: config.try_exists()?,
        locale_exists: target.join("ui.locale").try_exists()?,
    })
}

/// Copy artifacts and commit the legacy pointer last. Keep source files for rollback/version comparison.
pub fn relocate(
    location: &Location,
    target: &Path,
    overwrite: bool,
    current: &Config,
) -> Result<PathBuf, StorageError> {
    relocate_snapshot(location, target, overwrite, current).map(|(path, _)| path)
}

pub(crate) fn relocate_snapshot(
    location: &Location,
    target: &Path,
    overwrite: bool,
    current: &Config,
) -> Result<(PathBuf, Config), StorageError> {
    absolute(target)?;
    absolute(&location.implicit)?;
    absolute(&location.effective)?;
    if location.pointer != location.implicit.with_file_name("config.root") {
        return Err(invalid(
            "Configuration pointer must be beside the original configuration",
        ));
    }
    current.validate().map_err(StorageError::Invalid)?;
    let anchor = location
        .implicit
        .parent()
        .ok_or_else(|| invalid("Missing configuration anchor"))?;
    fs::create_dir_all(anchor)?;
    fs::create_dir_all(target)?;
    let target = fs::canonicalize(target)?;
    let anchor = fs::canonicalize(anchor)?;
    let destination = if target == anchor {
        location.implicit.clone()
    } else {
        target.join("config.toml")
    };
    if same_file(&location.effective, &destination) {
        if target == anchor {
            match fs::remove_file(&location.pointer) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        return Ok((destination, current.clone()));
    }
    let keep_config = !overwrite && destination.try_exists()?;
    let config = if keep_config {
        Storage::read_import(&destination)?
    } else {
        current.clone()
    };
    let data = toml::to_string_pretty(&config)
        .map_err(|_| invalid("Cannot serialize relocated configuration"))?
        .into_bytes();
    let source_locale = read_optional(&location.effective.with_file_name("ui.locale"))?;
    let locale_path = target.join("ui.locale");
    let locale = if !overwrite && locale_path.try_exists()? {
        None
    } else {
        source_locale
    };
    let mut changes = Vec::new();
    let mut verified = config;
    let commit = (|| -> Result<(), StorageError> {
        if !keep_config {
            replace_with_backup(&destination, &data, &mut changes)?;
        }
        if let Some(locale) = locale {
            replace_with_backup(&locale_path, &locale, &mut changes)?;
        }
        // Verify the destination is usable before redirecting future launches.
        verified = Storage::read_import(&destination)?;
        if target == anchor {
            match fs::remove_file(&location.pointer) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            replace_with_backup(
                &location.pointer,
                format!("{}\n", target.display()).as_bytes(),
                &mut changes,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = commit {
        let mut rollback_failed = false;
        for (path, previous) in changes.into_iter().rev() {
            let restored = match previous {
                Some(bytes) => atomic_write(&path, &bytes),
                None => match fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                },
            };
            rollback_failed |= restored.is_err();
        }
        return if rollback_failed {
            Err(invalid(
                "Relocation failed and rollback was incomplete; retained backups require inspection",
            ))
        } else {
            Err(error)
        };
    }
    Ok((destination, verified))
}

fn invalid(message: &str) -> StorageError {
    StorageError::Invalid(message.into())
}
fn absolute(path: &Path) -> Result<(), StorageError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(invalid("Configuration paths must be absolute"))
    }
}
fn same_file(left: &Path, right: &Path) -> bool {
    left == right
        || matches!((fs::canonicalize(left), fs::canonicalize(right)), (Ok(left), Ok(right)) if left == right)
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn replace_with_backup(
    path: &Path,
    bytes: &[u8],
    changes: &mut Vec<(PathBuf, Option<Vec<u8>>)>,
) -> Result<(), StorageError> {
    let previous = read_optional(path)?;
    if let Some(previous) = &previous {
        let mut backup = tempfile::Builder::new()
            .prefix("location-backup-")
            .suffix(".bak")
            .tempfile_in(path.parent().unwrap())?;
        backup.write_all(previous)?;
        backup.as_file().sync_all()?;
        backup.keep().map_err(|error| error.error)?;
    }
    changes.push((path.to_owned(), previous));
    atomic_write(path, bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let mut staged = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| invalid("Missing target directory"))?,
    )?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    if fs::read(path)? != bytes {
        return Err(invalid("Relocated file verification failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dir: &Path) -> (Location, Config) {
        let implicit = dir.join("original/config.toml");
        let config = Config::default();
        Storage::new(&implicit).save(&config).unwrap();
        fs::write(implicit.with_file_name("ui.locale"), "zh-CN\n").unwrap();
        (resolve(&implicit).unwrap(), config)
    }
    #[test]
    fn pointer_roundtrip_and_reset_preserve_both_config_and_locale_sources() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        let target = dir.path().join("custom");
        let destination = relocate(&location, &target, true, &config).unwrap();
        let redirected = resolve(&location.implicit).unwrap();
        assert_eq!(
            fs::canonicalize(&redirected.effective).unwrap(),
            fs::canonicalize(&destination).unwrap()
        );
        assert_eq!(
            fs::read_to_string(destination.with_file_name("ui.locale")).unwrap(),
            "zh-CN\n"
        );
        assert!(location.effective.is_file());
        relocate(
            &redirected,
            location.implicit.parent().unwrap(),
            true,
            &config,
        )
        .unwrap();
        assert!(!location.pointer.exists());
        assert_eq!(
            resolve(&location.implicit).unwrap().effective,
            location.implicit
        );
        assert!(destination.is_file());
    }
    #[test]
    fn keep_existing_is_validated_and_preserves_target_locale() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        let target = dir.path().join("custom");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("config.toml"), "invalid [ TOML").unwrap();
        assert!(relocate(&location, &target, false, &config).is_err());
        assert!(!location.pointer.exists());
        let existing = Config {
            traffic_monitor_enabled: false,
            ..Default::default()
        };
        Storage::new(target.join("config.toml"))
            .save(&existing)
            .unwrap();
        fs::write(target.join("ui.locale"), "en\n").unwrap();
        relocate(&location, &target, false, &config).unwrap();
        assert!(
            !Storage::read_import(&target.join("config.toml"))
                .unwrap()
                .traffic_monitor_enabled
        );
        assert_eq!(
            fs::read_to_string(target.join("ui.locale")).unwrap(),
            "en\n"
        );
    }
    #[test]
    fn pointer_failure_restores_existing_targets_and_never_deletes_source() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        fs::create_dir(&location.pointer).unwrap();
        let target = dir.path().join("custom");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("config.toml"), "original target bytes").unwrap();
        fs::write(target.join("ui.locale"), "en\n").unwrap();
        assert!(relocate(&location, &target, true, &config).is_err());
        assert_eq!(
            fs::read_to_string(target.join("config.toml")).unwrap(),
            "original target bytes"
        );
        assert_eq!(
            fs::read_to_string(target.join("ui.locale")).unwrap(),
            "en\n"
        );
        assert!(location.effective.is_file());
    }
    #[test]
    fn invalid_or_unavailable_pointer_never_silently_creates_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let (location, _) = fixture(dir.path());
        fs::write(&location.pointer, "relative/path\n").unwrap();
        assert!(resolve(&location.implicit).is_err());
        fs::write(
            &location.pointer,
            dir.path().join("missing").to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert!(resolve(&location.implicit).is_err());
        assert!(conflicts(&location, Path::new("relative")).is_err());
    }

    #[test]
    fn reset_removes_redundant_pointer_and_checks_nonstandard_anchor_filename() {
        let dir = tempfile::tempdir().unwrap();
        let implicit = dir.path().join("preview.toml");
        let config = Config::default();
        Storage::new(&implicit).save(&config).unwrap();
        let location = resolve(&implicit).unwrap();
        assert!(conflicts(&location, dir.path()).unwrap().config_exists);
        fs::write(&location.pointer, format!("{}\n", dir.path().display())).unwrap();
        relocate(&location, dir.path(), true, &config).unwrap();
        assert!(!location.pointer.exists());
        assert!(implicit.is_file());
    }

    #[test]
    fn pointer_failure_removes_new_target_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        fs::create_dir(&location.pointer).unwrap();
        let target = dir.path().join("custom");
        assert!(relocate(&location, &target, true, &config).is_err());
        assert!(!target.join("config.toml").exists());
        assert!(!target.join("ui.locale").exists());
        assert!(location.effective.is_file());
    }

    #[tokio::test]
    async fn controller_reads_and_updates_only_redirected_config() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        let source_bytes = fs::read(&location.implicit).unwrap();
        let target = dir.path().join("custom");
        let destination = relocate(&location, &target, true, &config).unwrap();
        let controller = crate::controller::Controller::open(
            location.implicit.clone(),
            dir.path().join("known_hosts"),
        )
        .unwrap();
        assert_eq!(controller.config_location().implicit, location.implicit);
        assert!(same_file(&controller.config_path(), &destination));
        controller.set_traffic_monitor(false).await.unwrap();
        assert!(
            !Storage::read_import(&destination)
                .unwrap()
                .traffic_monitor_enabled
        );
        assert_eq!(fs::read(&location.implicit).unwrap(), source_bytes);
    }

    #[test]
    fn keep_existing_preserves_exact_target_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (location, config) = fixture(dir.path());
        let target = dir.path().join("custom");
        fs::create_dir(&target).unwrap();
        let bytes = b"# Preserve comments and formatting\ntraffic_monitor_enabled = false\n";
        fs::write(target.join("config.toml"), bytes).unwrap();
        relocate(&location, &target, false, &config).unwrap();
        assert_eq!(fs::read(target.join("config.toml")).unwrap(), bytes);
    }

    #[tokio::test]
    async fn live_switch_changes_repository_invalidates_import_and_resets_to_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let (location, _) = fixture(dir.path());
        let controller = crate::controller::Controller::open(
            location.implicit.clone(),
            dir.path().join("known_hosts"),
        )
        .unwrap();
        let target = dir.path().join("custom");
        Storage::new(target.join("config.toml"))
            .save(&Config {
                traffic_monitor_enabled: false,
                ..Default::default()
            })
            .unwrap();
        let preview = controller
            .preview_import(target.join("config.toml"))
            .await
            .unwrap();
        assert!(
            controller
                .relocate_config(Some(target.clone()), false)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(!controller.snapshot().traffic_monitor_enabled);
        assert!(same_file(
            &controller.config_path(),
            &target.join("config.toml")
        ));
        assert!(controller.confirm_import(preview.token).await.is_err());
        assert!(
            Storage::read_import(&location.implicit)
                .unwrap()
                .traffic_monitor_enabled
        );
        controller.relocate_config(None, true).await.unwrap();
        assert!(!controller.snapshot().traffic_monitor_enabled);
        assert_eq!(controller.config_path(), location.implicit);
        assert!(!location.pointer.exists());
        assert!(target.join("config.toml").exists());
    }

    #[tokio::test]
    async fn failed_live_switch_keeps_repository_and_pending_import() {
        let dir = tempfile::tempdir().unwrap();
        let (location, _) = fixture(dir.path());
        let controller = crate::controller::Controller::open(
            location.implicit.clone(),
            dir.path().join("known_hosts"),
        )
        .unwrap();
        let preview = controller
            .preview_import(location.implicit.clone())
            .await
            .unwrap();
        fs::create_dir(&location.pointer).unwrap();
        assert!(
            controller
                .relocate_config(Some(dir.path().join("custom")), true)
                .await
                .is_err()
        );
        assert_eq!(controller.config_path(), location.implicit);
        assert!(controller.snapshot().traffic_monitor_enabled);
        controller.confirm_import(preview.token).await.unwrap();
    }
}
