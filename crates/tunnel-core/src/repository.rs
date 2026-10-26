use crate::{
    model::{Config, Group, Jumper, Tunnel},
    storage::{Storage, StorageError},
};

/// Owned by a single application mutex; failed writes never publish partial state.
pub struct Repository {
    storage: Storage,
    config: Config,
}

#[derive(serde::Serialize)]
pub struct BatchImport {
    pub imported: Vec<u64>,
    pub skipped: usize,
}

#[derive(serde::Deserialize)]
pub struct CommandImport {
    pub tunnels: Vec<Tunnel>,
    pub jumpers: Vec<Jumper>,
}

#[derive(serde::Serialize)]
pub struct TunnelBatchImport {
    pub imported: Vec<u64>,
    pub skipped: usize,
    pub created_jumpers: Vec<u64>,
}

impl Repository {
    pub(crate) fn from_committed_config(path: std::path::PathBuf, mut config: Config) -> Self {
        config.normalize_loaded();
        Self {
            storage: Storage::new(path),
            config,
        }
    }

    pub fn open(storage: Storage) -> Result<Self, StorageError> {
        let config = storage.load()?;
        Ok(Self { storage, config })
    }

    pub fn snapshot(&self) -> Config {
        self.config.clone()
    }

    pub fn path(&self) -> &std::path::Path {
        self.storage.path()
    }

    pub fn replace(&mut self, mut config: Config) -> Result<(), StorageError> {
        config.normalize_loaded();
        self.storage.save(&config)?;
        self.config = config;
        Ok(())
    }

    pub fn set_traffic_monitor(&mut self, enabled: bool) -> Result<(), StorageError> {
        self.transaction(|config| {
            config.traffic_monitor_enabled = enabled;
            Ok(())
        })
    }

    pub fn set_auto_run(&mut self, enabled: bool) -> Result<(), StorageError> {
        self.transaction(|config| {
            config.auto_run = enabled;
            Ok(())
        })
    }

    pub fn trust_host(&mut self, pin: crate::hostkeys::TrustedHost) -> Result<(), StorageError> {
        self.transaction(|config| {
            let key = russh::keys::PublicKey::from_openssh(&pin.public_key)
                .map_err(|_| "Invalid host key")?;
            config.trusted_hosts.retain(|old| {
                !(old.host.eq_ignore_ascii_case(&pin.host)
                    && old.port == pin.port
                    && russh::keys::PublicKey::from_openssh(&old.public_key)
                        .is_ok_and(|old_key| old_key.algorithm() == key.algorithm()))
            });
            config.trusted_hosts.push(pin);
            Ok(())
        })
    }

    fn transaction<T>(
        &mut self,
        edit: impl FnOnce(&mut Config) -> Result<T, String>,
    ) -> Result<T, StorageError> {
        let mut next = self.config.clone();
        let result = edit(&mut next).map_err(StorageError::Invalid)?;
        self.storage.save(&next)?;
        self.config = next;
        Ok(result)
    }

    pub fn save_jumper(&mut self, mut jumper: Jumper) -> Result<u64, StorageError> {
        self.transaction(|config| {
            if jumper.id == 0 {
                jumper.id = next_id(config.jumpers.iter().map(|j| j.id))?;
                config.jumpers.push(jumper.clone());
            } else {
                let old = config
                    .jumpers
                    .iter_mut()
                    .find(|j| j.id == jumper.id)
                    .ok_or("Jumper not found")?;
                *old = jumper.clone();
            }
            Ok(jumper.id)
        })
    }

    pub fn import_jumpers(&mut self, jumpers: Vec<Jumper>) -> Result<BatchImport, StorageError> {
        if jumpers.is_empty() || jumpers.len() > 2000 {
            return Err(StorageError::Invalid(
                "Select between 1 and 2000 jumpers".into(),
            ));
        }
        self.transaction(|config| {
            let signature = |jumper: &Jumper| {
                (
                    jumper.host.trim().to_lowercase(),
                    jumper.user.trim().to_owned(),
                    jumper.port,
                )
            };
            let mut existing: std::collections::HashSet<_> =
                config.jumpers.iter().map(signature).collect();
            let mut result = BatchImport {
                imported: vec![],
                skipped: 0,
            };
            for mut jumper in jumpers {
                jumper.host = jumper.host.trim().into();
                jumper.user = jumper.user.trim().into();
                jumper.name = jumper.name.trim().into();
                if !existing.insert(signature(&jumper)) {
                    result.skipped += 1;
                    continue;
                }
                jumper.id = next_id(config.jumpers.iter().map(|item| item.id))?;
                result.imported.push(jumper.id);
                config.jumpers.push(jumper);
            }
            Ok(result)
        })
    }

    pub fn delete_jumper(&mut self, id: u64) -> Result<(), StorageError> {
        self.transaction(|config| {
            if config.tunnels.iter().any(|t| t.jumper_ids.contains(&id)) {
                return Err("Jumper is used by a tunnel".into());
            }
            let index = config
                .jumpers
                .iter()
                .position(|j| j.id == id)
                .ok_or("Jumper not found")?;
            config.jumpers.remove(index);
            Ok(())
        })
    }

    pub fn save_tunnel(&mut self, mut tunnel: Tunnel) -> Result<u64, StorageError> {
        self.transaction(|config| {
            tunnel.group_id = 0;
            tunnel.status = "stopped".into();
            tunnel.last_error.clear();
            if tunnel.id == 0 {
                tunnel.id = next_id(config.tunnels.iter().map(|t| t.id))?;
                config.tunnels.push(tunnel.clone());
            } else {
                let old = config
                    .tunnels
                    .iter_mut()
                    .find(|t| t.id == tunnel.id)
                    .ok_or("Tunnel not found")?;
                *old = tunnel.clone();
            }
            Ok(tunnel.id)
        })
    }

    pub fn import_command_tunnels(
        &mut self,
        request: CommandImport,
    ) -> Result<TunnelBatchImport, StorageError> {
        if request.tunnels.is_empty()
            || request.tunnels.len() > 2000
            || request.jumpers.is_empty()
            || request.jumpers.len() > 128
        {
            return Err(StorageError::Invalid(
                "Invalid tunnel import size or empty jumper chain".into(),
            ));
        }
        self.transaction(|config| {
            let signature = |tunnel: &Tunnel| {
                (
                    tunnel.mode.clone(),
                    tunnel.local_host.trim().to_lowercase(),
                    tunnel.local_port,
                    if tunnel.mode == "dynamic" {
                        String::new()
                    } else {
                        tunnel.remote_host.trim().to_lowercase()
                    },
                    if tunnel.mode == "dynamic" {
                        0
                    } else {
                        tunnel.remote_port
                    },
                )
            };
            let mut existing: std::collections::HashSet<_> =
                config.tunnels.iter().map(signature).collect();
            let mut result = TunnelBatchImport {
                imported: vec![],
                skipped: 0,
                created_jumpers: vec![],
            };
            let fresh: Vec<_> = request
                .tunnels
                .into_iter()
                .filter(|tunnel| {
                    if existing.insert(signature(tunnel)) {
                        true
                    } else {
                        result.skipped += 1;
                        false
                    }
                })
                .collect();
            if fresh.is_empty() {
                return Ok(result);
            }
            let mut ids = Vec::new();
            for mut jumper in request.jumpers {
                if jumper.id != 0 {
                    if !config.jumpers.iter().any(|item| item.id == jumper.id) {
                        return Err("Selected jumper no longer exists".into());
                    }
                    ids.push(jumper.id);
                    continue;
                }
                jumper.host = jumper.host.trim().into();
                jumper.user = jumper.user.trim().into();
                if let Some(existing) = config.jumpers.iter().find(|item| {
                    item.host.eq_ignore_ascii_case(&jumper.host)
                        && item.user == jumper.user
                        && item.port == jumper.port
                }) {
                    ids.push(existing.id);
                } else {
                    jumper.id = next_id(config.jumpers.iter().map(|item| item.id))?;
                    ids.push(jumper.id);
                    result.created_jumpers.push(jumper.id);
                    config.jumpers.push(jumper);
                }
            }
            for mut tunnel in fresh {
                tunnel.id = next_id(config.tunnels.iter().map(|item| item.id))?;
                tunnel.jumper_ids = ids.clone();
                tunnel.group_id = 0;
                tunnel.status = "stopped".into();
                tunnel.last_error.clear();
                result.imported.push(tunnel.id);
                config.tunnels.push(tunnel);
            }
            Ok(result)
        })
    }

    pub fn delete_tunnel(&mut self, id: u64) -> Result<(), StorageError> {
        self.transaction(|config| {
            let index = config
                .tunnels
                .iter()
                .position(|t| t.id == id)
                .ok_or("Tunnel not found")?;
            config.tunnels.remove(index);
            for group in &mut config.groups {
                group.tunnel_ids.retain(|member| *member != id);
            }
            Ok(())
        })
    }

    pub fn save_group(&mut self, mut group: Group) -> Result<u64, StorageError> {
        self.transaction(|config| {
            group.name = group.name.trim().into();
            group.tunnel_ids.sort_unstable();
            group.tunnel_ids.dedup();
            if group.id == 0 {
                group.id = next_id(config.groups.iter().map(|g| g.id))?;
                config.groups.push(group.clone());
            } else {
                let old = config
                    .groups
                    .iter_mut()
                    .find(|g| g.id == group.id)
                    .ok_or("Group not found")?;
                *old = group.clone();
            }
            Ok(group.id)
        })
    }

    pub fn reorder_groups(&mut self, ids: Vec<u64>) -> Result<(), StorageError> {
        self.transaction(|config| {
            let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
            if ids.len() != config.groups.len()
                || unique.len() != ids.len()
                || config
                    .groups
                    .iter()
                    .any(|group| !unique.contains(&group.id))
            {
                return Err("Group order must contain every custom group exactly once".into());
            }
            let mut groups: std::collections::HashMap<_, _> = config
                .groups
                .drain(..)
                .map(|group| (group.id, group))
                .collect();
            config.groups = ids
                .into_iter()
                .map(|id| groups.remove(&id).unwrap())
                .collect();
            Ok(())
        })
    }

    pub fn delete_group(&mut self, id: u64) -> Result<(), StorageError> {
        self.transaction(|config| {
            let index = config
                .groups
                .iter()
                .position(|g| g.id == id)
                .ok_or("Group not found")?;
            config.groups.remove(index);
            Ok(())
        })
    }
}

fn next_id(ids: impl Iterator<Item = u64>) -> Result<u64, String> {
    let max = ids.max().unwrap_or(0);
    if max >= 9_007_199_254_740_991 {
        return Err("ID space exhausted".into());
    }
    Ok(max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated(path: &std::path::Path) -> Repository {
        let mut repo = Repository::open(Storage::new(path)).unwrap();
        repo.save_jumper(Jumper {
            host: "localhost".into(),
            port: 22,
            user: "test".into(),
            auth_type: "ssh_agent".into(),
            ..Default::default()
        })
        .unwrap();
        repo.save_tunnel(Tunnel {
            name: "Example".into(),
            jumper_ids: vec![1],
            local_port: 8080,
            remote_port: 80,
            ..Default::default()
        })
        .unwrap();
        repo
    }

    #[test]
    fn overlapping_groups_and_removal_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        for name in ["Work", "Services"] {
            repo.save_group(Group {
                name: name.into(),
                tunnel_ids: vec![1],
                ..Default::default()
            })
            .unwrap();
        }
        repo.save_group(Group {
            id: 1,
            name: "Work".into(),
            tunnel_ids: vec![],
        })
        .unwrap();
        let mut repo = Repository::open(Storage::new(&path)).unwrap();
        assert!(repo.snapshot().groups[0].tunnel_ids.is_empty());
        assert_eq!(repo.snapshot().groups[1].tunnel_ids, vec![1]);
        assert!(repo.delete_jumper(1).is_err());
        repo.delete_tunnel(1).unwrap();
        assert!(
            repo.snapshot()
                .groups
                .iter()
                .all(|g| g.tunnel_ids.is_empty())
        );
        repo.delete_jumper(1).unwrap();
    }

    #[test]
    fn validation_failure_is_transactional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(
            repo.save_group(Group {
                name: "Invalid".into(),
                tunnel_ids: vec![999],
                ..Default::default()
            })
            .is_err()
        );
        assert!(repo.snapshot().groups.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn removing_imported_legacy_membership_does_not_resurrect_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let repo = populated(&path);
        let mut old = repo.snapshot();
        old.tunnels[0].group_id = 1;
        old.groups.push(Group {
            id: 1,
            name: "Legacy".into(),
            tunnel_ids: vec![],
        });
        Storage::new(&path).save(&old).unwrap();
        let mut migrated = Repository::open(Storage::new(&path)).unwrap();
        assert_eq!(migrated.snapshot().groups[0].tunnel_ids, vec![1]);
        migrated
            .save_group(Group {
                id: 1,
                name: "Legacy".into(),
                tunnel_ids: vec![],
            })
            .unwrap();
        let reloaded = Repository::open(Storage::new(&path)).unwrap();
        assert!(reloaded.snapshot().groups[0].tunnel_ids.is_empty());
        assert_eq!(reloaded.snapshot().tunnels[0].group_id, 0);
    }

    #[test]
    fn group_order_is_persistent_and_rejects_invalid_or_stale_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        for name in ["A", "B", "C"] {
            repo.save_group(Group {
                name: name.into(),
                tunnel_ids: vec![1],
                ..Default::default()
            })
            .unwrap();
        }
        repo.reorder_groups(vec![3, 1, 2]).unwrap();
        let mut repo = Repository::open(Storage::new(&path)).unwrap();
        assert_eq!(
            repo.snapshot()
                .groups
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        assert!(
            repo.snapshot()
                .groups
                .iter()
                .all(|g| g.tunnel_ids == vec![1])
        );
        let before = std::fs::read(&path).unwrap();
        for invalid in [
            vec![],
            vec![1, 2],
            vec![1, 1, 2],
            vec![1, 2, 9],
            vec![1, 2, 3, 4],
        ] {
            assert!(repo.reorder_groups(invalid).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(repo.snapshot().groups[0].id, 3);
        }
    }

    #[test]
    fn command_import_reuses_credentials_deduplicates_and_never_leaves_orphan_jumpers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        let existing = repo.snapshot().jumpers[0].clone();
        let altered = Jumper {
            id: 0,
            password: "ignored-fixture-value".into(),
            ..existing.clone()
        };
        let tunnel = Tunnel {
            id: 99,
            name: "Imported".into(),
            local_port: 9090,
            remote_port: 80,
            status: "running".into(),
            ..Default::default()
        };
        let result = repo
            .import_command_tunnels(CommandImport {
                jumpers: vec![altered],
                tunnels: vec![tunnel.clone(), tunnel.clone()],
            })
            .unwrap();
        assert_eq!(result.imported, vec![2]);
        assert_eq!(result.skipped, 1);
        assert!(result.created_jumpers.is_empty());
        assert!(repo.snapshot().jumpers[0] == existing);
        assert_eq!(repo.snapshot().tunnels[1].jumper_ids, vec![1]);
        assert_eq!(repo.snapshot().tunnels[1].status, "stopped");
        let before = std::fs::read(&path).unwrap();
        let new_host = Jumper {
            id: 0,
            host: "new.example".into(),
            ..existing
        };
        let bad = Tunnel {
            local_port: 0,
            ..tunnel.clone()
        };
        assert!(
            repo.import_command_tunnels(CommandImport {
                jumpers: vec![new_host.clone()],
                tunnels: vec![bad]
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(repo.snapshot().jumpers.len(), 1);
        let duplicate = repo
            .import_command_tunnels(CommandImport {
                jumpers: vec![new_host],
                tunnels: vec![tunnel],
            })
            .unwrap();
        assert!(duplicate.imported.is_empty());
        assert_eq!(duplicate.skipped, 1);
        assert_eq!(
            Repository::open(Storage::new(path))
                .unwrap()
                .snapshot()
                .jumpers
                .len(),
            1
        );
    }

    #[test]
    fn batch_import_deduplicates_without_overwriting_and_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        let original = repo.snapshot().jumpers[0].clone();
        let mut duplicate = original.clone();
        duplicate.name = "Do not overwrite".into();
        duplicate.host = " LOCALHOST ".into();
        let new = Jumper {
            id: 999,
            host: "new.example".into(),
            ..original.clone()
        };
        let result = repo
            .import_jumpers(vec![duplicate, new.clone(), new])
            .unwrap();
        assert_eq!(result.skipped, 2);
        assert_eq!(result.imported, vec![2]);
        assert!(repo.snapshot().jumpers[0] == original);
        let before = std::fs::read(&path).unwrap();
        let good = Jumper {
            host: "another.example".into(),
            ..original.clone()
        };
        let bad = Jumper {
            host: "invalid.example".into(),
            user: String::new(),
            ..original
        };
        assert!(repo.import_jumpers(vec![good, bad]).is_err());
        assert_eq!(repo.snapshot().jumpers.len(), 2);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn traffic_preference_persists_and_failed_writes_preserve_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        repo.set_traffic_monitor(false).unwrap();
        let mut repo = Repository::open(Storage::new(&path)).unwrap();
        assert!(!repo.snapshot().traffic_monitor_enabled);
        std::fs::rename(&path, dir.path().join("saved.toml")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(repo.set_traffic_monitor(true).is_err());
        assert!(!repo.snapshot().traffic_monitor_enabled);
        assert_eq!(repo.snapshot().tunnels.len(), 1);
    }

    #[test]
    fn write_failure_does_not_publish_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut repo = populated(&path);
        let original = dir.path().join("original.toml");
        std::fs::rename(&path, original).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(repo.delete_tunnel(1).is_err());
        assert_eq!(repo.snapshot().tunnels.len(), 1);
    }
}
