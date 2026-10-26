use crate::{
    hostkeys::{HostKeys, TrustedHost},
    model::{Config, Group, Jumper, Tunnel},
    repository::Repository,
    runtime::{Runtime, RuntimeEvent, TunnelSnapshot},
    ssh::Hop,
    storage::{Storage, StorageError},
};
use std::{path::PathBuf, sync::Mutex};
use tokio::sync::{Mutex as AsyncMutex, broadcast};

/// Serializes native commands so trust decisions, configuration edits and restarts cannot race.
pub struct Controller {
    probes: crate::probe_session::Sessions,
    location: crate::location::Location,
    repository: Mutex<Repository>,
    runtime: Runtime,
    operations: AsyncMutex<()>,
    known_hosts: PathBuf,
    pending_import: Mutex<Option<(u64, Config)>>,
    import_generation: std::sync::atomic::AtomicU64,
}

#[derive(serde::Serialize)]
pub struct ImportPreview {
    pub token: u64,
    pub jumpers: usize,
    pub tunnels: usize,
    pub groups: usize,
    pub trusted_hosts: usize,
    pub bypass_jumpers: usize,
    pub auto_start: usize,
    pub auto_run: bool,
}

impl Controller {
    pub fn open(config: PathBuf, known_hosts: PathBuf) -> Result<Self, StorageError> {
        let location = crate::location::resolve(&config)?;
        Ok(Self {
            probes: Default::default(),
            repository: Mutex::new(Repository::open(Storage::new(&location.effective))?),
            location,
            runtime: Runtime::default(),
            operations: AsyncMutex::new(()),
            known_hosts,
            pending_import: Mutex::new(None),
            import_generation: Default::default(),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.runtime.subscribe()
    }
    pub fn record_activity(&self, operation: &str, success: bool) {
        self.runtime.activity(operation, success);
    }
    pub fn activity_logs(&self) -> Vec<RuntimeEvent> {
        self.runtime.journal()
    }

    pub async fn begin_probe(&self, request: crate::probe_session::Request) -> Result<u64, String> {
        let _guard = self.operations.lock().await;
        self.probes.begin(request)
    }

    pub async fn cancel_probe(&self, token: u64) {
        let _guard = self.operations.lock().await;
        self.probes.cancel(Some(token));
    }

    pub async fn run_probe(&self, token: u64) -> Result<crate::probe::Report, String> {
        let (request, cancel, hops) = {
            let _guard = self.operations.lock().await;
            let (request, cancel) = self.probes.claim(token)?;
            let config = self.snapshot();
            let hops = request
                .jumpers
                .iter()
                .map(|jumper| {
                    Ok(Hop {
                        jumper: jumper.clone(),
                        trusted_key: None,
                        known_keys: HostKeys::load(
                            &self.known_hosts,
                            &jumper.host,
                            jumper.port,
                            &config.trusted_hosts,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, String>>();
            match hops {
                Ok(hops) => (request, cancel, hops),
                Err(error) => {
                    self.probes.finish(token, None);
                    return Err(error);
                }
            }
        };
        let report = if request.diagnostics {
            crate::probe::diagnose_connection(request.tunnel.as_ref(), &hops, &cancel).await
        } else {
            crate::probe::test_connection(request.tunnel.as_ref(), &hops, &cancel).await
        };
        self.probes.finish(token, report.challenge.clone());
        Ok(report)
    }

    pub async fn approve_probe_key(&self, token: u64) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        let challenge = self.probes.challenge(token)?;
        let mut repository = self.repository.lock().expect("Configuration lock poisoned");
        let keys = HostKeys::load(
            &self.known_hosts,
            &challenge.host,
            challenge.port,
            &repository.snapshot().trusted_hosts,
        )?;
        let key = russh::keys::PublicKey::from_openssh(&challenge.public_key)
            .map_err(|_| "Invalid server key")?;
        if challenge
            .authority_fingerprint
            .as_ref()
            .is_some_and(|fingerprint| {
                keys.revoked.iter().any(|key| {
                    key.fingerprint(russh::keys::HashAlg::Sha256).to_string() == *fingerprint
                })
            })
        {
            return Err("Server certificate authority has been revoked".into());
        }
        if matches!(
            keys.verify(&challenge.host, &key.into()),
            crate::hostkeys::Verification::Revoked
        ) {
            return Err("Server key has been revoked".into());
        }
        repository
            .trust_host(TrustedHost {
                host: challenge.host,
                port: challenge.port,
                public_key: challenge.public_key,
            })
            .map_err(|error| error.to_string())?;
        self.probes.finish(token, None);
        Ok(())
    }
    pub fn config_location(&self) -> crate::location::Location {
        crate::location::Location {
            effective: self.config_path(),
            ..self.location.clone()
        }
    }

    pub async fn relocate_config(
        &self,
        target: Option<PathBuf>,
        overwrite: bool,
    ) -> Result<Vec<String>, String> {
        let _guard = self.operations.lock().await;
        let location = self.config_location();
        let target = target.unwrap_or_else(|| location.implicit.parent().unwrap().to_owned());
        let (path, config) =
            crate::location::relocate_snapshot(&location, &target, overwrite, &self.snapshot())
                .map_err(|error| error.to_string())?;
        // All fallible file work precedes stopping sessions or publishing the new repository.
        self.probes.cancel(None);
        self.probes.wait_idle().await;
        self.runtime.stop_all().await;
        for state in self.runtime.snapshots() {
            self.runtime.forget(state.tunnel_id).await;
        }
        *self.repository.lock().expect("Configuration lock poisoned") =
            Repository::from_committed_config(path, config);
        self.runtime.clear_journal();
        *self.pending_import.lock().expect("Import lock poisoned") = None;
        Ok(self.start_automatically_inner().await)
    }
    pub fn config_path(&self) -> PathBuf {
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .path()
            .to_owned()
    }

    pub async fn set_auto_run(
        &self,
        enabled: bool,
        platform: &impl crate::login::LoginStartup,
    ) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        let before = platform.snapshot()?;
        platform.apply(enabled)?;
        if let Err(error) = self
            .repository
            .lock()
            .expect("Configuration lock poisoned")
            .set_auto_run(enabled)
        {
            if platform.restore(before).is_err() {
                return Err(format!(
                    "{error}; failed to restore login startup registration"
                ));
            }
            return Err(error.to_string());
        }
        Ok(())
    }

    pub async fn reconcile_auto_run(
        &self,
        platform: &impl crate::login::LoginStartup,
    ) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        let desired = self.snapshot().auto_run;
        // Refresh moved executable paths, and remove stale entries when disabled.
        platform.apply(desired)
    }

    pub async fn preview_import(&self, path: PathBuf) -> Result<ImportPreview, String> {
        let _guard = self.operations.lock().await;
        *self.pending_import.lock().expect("Import lock poisoned") = None;
        if !path.is_absolute() {
            return Err("Configuration path must be absolute".into());
        }
        let config = Storage::read_import(&path).map_err(|error| error.to_string())?;
        let token = self
            .import_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let preview = ImportPreview {
            token,
            jumpers: config.jumpers.len(),
            tunnels: config.tunnels.len(),
            groups: config.groups.len(),
            trusted_hosts: config.trusted_hosts.len(),
            bypass_jumpers: config
                .jumpers
                .iter()
                .filter(|j| j.bypass_host_verification)
                .count(),
            auto_start: config.tunnels.iter().filter(|t| t.auto_start).count(),
            auto_run: config.auto_run,
        };
        *self.pending_import.lock().expect("Import lock poisoned") = Some((token, config));
        Ok(preview)
    }

    pub async fn cancel_import(&self) {
        let _guard = self.operations.lock().await;
        *self.pending_import.lock().expect("Import lock poisoned") = None;
    }

    pub async fn confirm_import(&self, token: u64) -> Result<Vec<String>, String> {
        let _guard = self.operations.lock().await;
        let config = {
            let pending = self.pending_import.lock().expect("Import lock poisoned");
            if pending
                .as_ref()
                .is_none_or(|(current, _)| *current != token)
            {
                return Err("Import preview is no longer active".into());
            }
            pending.as_ref().unwrap().1.clone()
        };
        // Commit before interrupting connections. A failed disk write leaves them untouched.
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .replace(config)
            .map_err(|error| error.to_string())?;
        *self.pending_import.lock().expect("Import lock poisoned") = None;
        self.probes.cancel(None);
        self.probes.wait_idle().await;
        self.runtime.stop_all().await;
        for state in self.runtime.snapshots() {
            self.runtime.forget(state.tunnel_id).await;
        }
        self.runtime.clear_journal();
        Ok(self.start_automatically_inner().await)
    }

    pub async fn export_config(&self, path: PathBuf) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        if !path.is_absolute() {
            return Err("Configuration path must be absolute".into());
        }
        let repository = self.repository.lock().expect("Configuration lock poisoned");
        if path == repository.path()
            || (path.exists()
                && std::fs::canonicalize(&path).ok()
                    == std::fs::canonicalize(repository.path()).ok())
        {
            return Err("Choose an export path different from the active configuration".into());
        }
        Storage::new(path)
            .save(&repository.snapshot())
            .map_err(|error| error.to_string())
    }
    pub async fn set_traffic_monitor(&self, enabled: bool) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .set_traffic_monitor(enabled)
            .map_err(|error| error.to_string())
    }
    pub fn runtime_state(&self) -> Vec<TunnelSnapshot> {
        self.runtime.snapshots()
    }
    pub fn logs(&self, id: u64) -> Vec<RuntimeEvent> {
        self.runtime.logs(id)
    }

    pub fn snapshot(&self) -> Config {
        let mut config = self
            .repository
            .lock()
            .expect("Configuration lock poisoned")
            .snapshot();
        let states = self.runtime.snapshots();
        for tunnel in &mut config.tunnels {
            if let Some(state) = states.iter().find(|state| state.tunnel_id == tunnel.id) {
                tunnel.status = state.status.clone();
                tunnel.last_error = state.last_error.clone();
            }
        }
        config
    }

    fn ensure_idle(&self, id: u64) -> Result<(), String> {
        if self.runtime.snapshots().iter().any(|state| {
            state.tunnel_id == id && !["stopped", "error"].contains(&state.status.as_str())
        }) {
            return Err("Stop the tunnel before changing its connection settings".into());
        }
        Ok(())
    }

    async fn start_inner(&self, id: u64) -> Result<(), String> {
        let config = self
            .repository
            .lock()
            .expect("Configuration lock poisoned")
            .snapshot();
        let tunnel = config
            .tunnels
            .iter()
            .find(|tunnel| tunnel.id == id)
            .ok_or("Tunnel not found")?
            .clone();
        let mut hops = Vec::new();
        for id in &tunnel.jumper_ids {
            let jumper = config
                .jumpers
                .iter()
                .find(|jumper| jumper.id == *id)
                .ok_or("Jumper not found")?
                .clone();
            let known_keys = HostKeys::load(
                &self.known_hosts,
                &jumper.host,
                jumper.port,
                &config.trusted_hosts,
            )?;
            hops.push(Hop {
                jumper,
                trusted_key: None,
                known_keys,
            });
        }
        self.runtime.start(tunnel, hops).await
    }

    pub async fn start(&self, id: u64) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.start_inner(id).await
    }
    /// Launch all marked tunnels; handshakes run in independent runtime tasks.
    pub async fn start_automatically(&self) -> Vec<String> {
        let _guard = self.operations.lock().await;
        self.start_automatically_inner().await
    }
    async fn start_automatically_inner(&self) -> Vec<String> {
        let ids: Vec<_> = self
            .snapshot()
            .tunnels
            .iter()
            .filter(|tunnel| tunnel.auto_start)
            .map(|tunnel| tunnel.id)
            .collect();
        let mut errors = Vec::new();
        for id in ids {
            if let Err(error) = self.start_inner(id).await {
                errors.push(format!("Tunnel #{id}: {error}"));
            }
        }
        errors
    }
    pub async fn stop(&self, id: u64) {
        let _guard = self.operations.lock().await;
        self.runtime.stop(id).await;
    }
    pub async fn stop_all(&self) {
        let _guard = self.operations.lock().await;
        self.probes.cancel(None);
        self.probes.wait_idle().await;
        self.runtime.stop_all().await;
    }

    pub fn pending_trust(&self) -> Vec<RuntimeEvent> {
        self.runtime
            .snapshots()
            .iter()
            .filter(|state| state.status == "awaiting_trust")
            .filter_map(|state| {
                self.runtime
                    .logs(state.tunnel_id)
                    .into_iter()
                    .rev()
                    .find(|event| event.generation == state.generation && event.challenge.is_some())
            })
            .collect()
    }

    pub async fn resolve_host_key(
        &self,
        id: u64,
        generation: u64,
        accept: bool,
    ) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        let event = self
            .pending_trust()
            .into_iter()
            .find(|event| event.tunnel_id == id && event.generation == generation)
            .ok_or("Host-key request is no longer active")?;
        if !accept {
            self.runtime.stop(id).await;
            return Ok(());
        }
        let challenge = event.challenge.ok_or("Missing host-key challenge")?;
        {
            let mut repository = self.repository.lock().expect("Configuration lock poisoned");
            let config = repository.snapshot();
            let tunnel = config
                .tunnels
                .iter()
                .find(|tunnel| tunnel.id == id)
                .ok_or("Tunnel not found")?;
            let jumper = config
                .jumpers
                .iter()
                .find(|jumper| jumper.id == challenge.jumper_id)
                .ok_or("Jumper not found")?;
            if !tunnel.jumper_ids.contains(&jumper.id)
                || !jumper.host.eq_ignore_ascii_case(&challenge.host)
                || jumper.port != challenge.port
            {
                return Err("Connection settings changed; retry the connection".into());
            }
            // Re-read revocations immediately before accepting. The UI never supplies a replacement key.
            let keys = HostKeys::load(
                &self.known_hosts,
                &challenge.host,
                challenge.port,
                &config.trusted_hosts,
            )?;
            let key = russh::keys::PublicKey::from_openssh(&challenge.public_key)
                .map_err(|_| "Invalid server key")?;
            if challenge
                .authority_fingerprint
                .as_ref()
                .is_some_and(|fingerprint| {
                    keys.revoked.iter().any(|key| {
                        key.fingerprint(russh::keys::HashAlg::Sha256).to_string() == *fingerprint
                    })
                })
            {
                return Err("Server certificate authority has been revoked".into());
            }
            if matches!(
                keys.verify(&challenge.host, &key.into()),
                crate::hostkeys::Verification::Revoked
            ) {
                return Err("Server key has been revoked".into());
            }
            repository
                .trust_host(TrustedHost {
                    host: challenge.host,
                    port: challenge.port,
                    public_key: challenge.public_key,
                })
                .map_err(|error| error.to_string())?;
        }
        self.runtime.stop(id).await;
        self.start_inner(id).await
    }

    pub async fn save_tunnel(&self, tunnel: Tunnel) -> Result<u64, String> {
        let _guard = self.operations.lock().await;
        self.ensure_idle(tunnel.id)?;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .save_tunnel(tunnel)
            .map_err(|error| error.to_string())
    }
    pub async fn delete_tunnel(&self, id: u64) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.runtime.stop(id).await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .delete_tunnel(id)
            .map_err(|error| error.to_string())?;
        self.runtime.forget(id).await;
        Ok(())
    }
    pub async fn save_jumper(&self, jumper: Jumper) -> Result<u64, String> {
        let _guard = self.operations.lock().await;
        for tunnel in self
            .snapshot()
            .tunnels
            .iter()
            .filter(|tunnel| tunnel.jumper_ids.contains(&jumper.id))
        {
            self.ensure_idle(tunnel.id)?;
        }
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .save_jumper(jumper)
            .map_err(|error| error.to_string())
    }
    pub async fn import_jumpers(
        &self,
        jumpers: Vec<Jumper>,
    ) -> Result<crate::repository::BatchImport, String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .import_jumpers(jumpers)
            .map_err(|error| error.to_string())
    }

    pub async fn import_command_tunnels(
        &self,
        request: crate::repository::CommandImport,
    ) -> Result<crate::repository::TunnelBatchImport, String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .import_command_tunnels(request)
            .map_err(|error| error.to_string())
    }
    pub async fn delete_jumper(&self, id: u64) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .delete_jumper(id)
            .map_err(|error| error.to_string())
    }
    pub async fn save_group(&self, group: Group) -> Result<u64, String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .save_group(group)
            .map_err(|error| error.to_string())
    }
    pub async fn reorder_groups(&self, ids: Vec<u64>) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .reorder_groups(ids)
            .map_err(|error| error.to_string())
    }
    pub async fn delete_group(&self, id: u64) -> Result<(), String> {
        let _guard = self.operations.lock().await;
        self.repository
            .lock()
            .expect("Configuration lock poisoned")
            .delete_group(id)
            .map_err(|error| error.to_string())
    }
}
