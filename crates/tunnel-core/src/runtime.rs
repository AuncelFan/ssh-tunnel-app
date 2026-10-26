use crate::{
    forward::{RemoteListener, serve_local},
    model::Tunnel,
    reconnect::{self, Policy},
    ssh::{ConnectError, Hop, HostKeyChallenge, connect_chain, connect_remote_chain},
    traffic::{Counters, Totals},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex as AsyncMutex, broadcast, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeEvent {
    pub timestamp_ms: u64,
    pub tunnel_id: u64,
    pub generation: u64,
    pub kind: String,
    pub message: String,
    pub challenge: Option<HostKeyChallenge>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TunnelSnapshot {
    pub tunnel_id: u64,
    pub generation: u64,
    pub status: String,
    pub last_error: String,
    pub retry_attempt: u64,
    pub traffic: Totals,
    pub traffic_last_hour: Totals,
}

struct State {
    snapshot: TunnelSnapshot,
    counters: Arc<Counters>,
    logs: std::collections::VecDeque<RuntimeEvent>,
}

struct Running {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone)]
struct Shared {
    journal: Arc<Mutex<std::collections::VecDeque<RuntimeEvent>>>,
    states: Arc<Mutex<HashMap<u64, State>>>,
    events: broadcast::Sender<RuntimeEvent>,
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl Shared {
    fn emit(&self, id: u64, kind: &str, message: String, challenge: Option<HostKeyChallenge>) {
        let mut event = RuntimeEvent {
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            tunnel_id: id,
            generation: 0,
            kind: kind.into(),
            message,
            challenge,
        };
        if let Some(state) = self
            .states
            .lock()
            .expect("Runtime state lock poisoned")
            .get_mut(&id)
        {
            event.generation = state.snapshot.generation;
            if state.logs.len() == 300 {
                state.logs.pop_front();
            }
            state.logs.push_back(event.clone());
        }
        {
            let mut journal = self.journal.lock().expect("Journal lock poisoned");
            if journal.len() == 500 {
                journal.pop_front();
            }
            journal.push_back(event.clone());
        }
        let _ = self.events.send(event);
    }

    fn status(&self, id: u64, status: &str, error: &str, attempt: u64) {
        if let Some(state) = self
            .states
            .lock()
            .expect("Runtime state lock poisoned")
            .get_mut(&id)
        {
            state.snapshot.status = status.into();
            state.snapshot.last_error = error.into();
            state.snapshot.retry_attempt = attempt;
        }
        self.emit(id, "status", status.into(), None);
    }
}

pub struct Runtime {
    running: AsyncMutex<HashMap<u64, Running>>,
    shared: Shared,
}

impl Default for Runtime {
    fn default() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            running: AsyncMutex::new(HashMap::new()),
            shared: Shared {
                journal: Default::default(),
                states: Default::default(),
                events,
                generation: Default::default(),
            },
        }
    }
}

impl Runtime {
    pub fn activity(&self, operation: &str, success: bool) {
        self.shared.emit(
            0,
            if success { "action" } else { "action_error" },
            operation.into(),
            None,
        );
    }
    pub fn journal(&self) -> Vec<RuntimeEvent> {
        self.shared
            .journal
            .lock()
            .expect("Journal lock poisoned")
            .iter()
            .rev()
            .cloned()
            .collect()
    }
    pub fn clear_journal(&self) {
        self.shared
            .journal
            .lock()
            .expect("Journal lock poisoned")
            .clear();
    }
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.shared.events.subscribe()
    }

    pub fn snapshots(&self) -> Vec<TunnelSnapshot> {
        let states = self
            .shared
            .states
            .lock()
            .expect("Runtime state lock poisoned");
        let mut snapshots: Vec<_> = states
            .values()
            .map(|state| {
                let mut snapshot = state.snapshot.clone();
                snapshot.traffic = state.counters.snapshot();
                snapshot.traffic_last_hour = state.counters.last_hour();
                snapshot
            })
            .collect();
        snapshots.sort_by_key(|snapshot| snapshot.tunnel_id);
        snapshots
    }

    pub fn logs(&self, id: u64) -> Vec<RuntimeEvent> {
        self.shared
            .states
            .lock()
            .expect("Runtime state lock poisoned")
            .get(&id)
            .map(|state| state.logs.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn start(&self, tunnel: Tunnel, hops: Vec<Hop>) -> Result<(), String> {
        let policy = Policy::from_tunnel(&tunnel)?;
        if tunnel.id == 0
            || hops.is_empty()
            || !["local", "remote", "dynamic"].contains(&tunnel.mode.as_str())
        {
            return Err("Invalid tunnel or empty jumper chain".into());
        }
        let mut running = self.running.lock().await;
        if running
            .get(&tunnel.id)
            .is_some_and(|entry| !entry.task.is_finished())
        {
            return Err("Tunnel is already active".into());
        }
        running.remove(&tunnel.id);
        let counters = {
            let mut states = self
                .shared
                .states
                .lock()
                .expect("Runtime state lock poisoned");
            let state = states.entry(tunnel.id).or_insert_with(|| State {
                snapshot: TunnelSnapshot {
                    tunnel_id: tunnel.id,
                    generation: 0,
                    status: "stopped".into(),
                    last_error: String::new(),
                    retry_attempt: 0,
                    traffic: Totals::default(),
                    traffic_last_hour: Totals::default(),
                },
                counters: Arc::new(Counters::default()),
                logs: Default::default(),
            });
            state.snapshot.generation = self
                .shared
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            state.counters.clone()
        };
        let cancel = CancellationToken::new();
        self.shared.status(tunnel.id, "connecting", "", 0);
        let shared = self.shared.clone();
        let worker_cancel = cancel.clone();
        let id = tunnel.id;
        let task = tokio::spawn(async move {
            supervise(tunnel, hops, policy, counters, worker_cancel, shared).await;
        });
        running.insert(id, Running { cancel, task });
        Ok(())
    }

    pub async fn stop(&self, id: u64) {
        let mut running = self.running.lock().await;
        if let Some(mut entry) = running.remove(&id) {
            entry.cancel.cancel();
            let _ = (&mut entry.task).await;
        }
        self.shared.status(id, "stopped", "", 0);
    }

    pub async fn stop_all(&self) {
        let mut running = self.running.lock().await;
        for entry in running.values() {
            entry.cancel.cancel();
        }
        for (id, mut entry) in running.drain() {
            let _ = (&mut entry.task).await;
            self.shared.status(id, "stopped", "", 0);
        }
    }

    pub async fn forget(&self, id: u64) {
        self.stop(id).await;
        self.shared
            .states
            .lock()
            .expect("Runtime state lock poisoned")
            .remove(&id);
    }
}

async fn supervise(
    tunnel: Tunnel,
    hops: Vec<Hop>,
    policy: Policy,
    counters: Arc<Counters>,
    cancel: CancellationToken,
    shared: Shared,
) {
    let mut attempt = 0u64;
    let mut outage = Instant::now();
    loop {
        if attempt > 0 {
            shared.status(tunnel.id, "connecting", "", attempt);
        }
        let result = run_once(&tunnel, &hops, counters.clone(), &cancel, &shared).await;
        if cancel.is_cancelled() {
            shared.status(tunnel.id, "stopped", "", 0);
            return;
        }
        let (error, was_running) = match result {
            Ok(()) => ("SSH connection closed".into(), true),
            Err((ConnectError::HostKey(challenge), _)) => {
                shared.status(tunnel.id, "awaiting_trust", "", 0);
                shared.emit(
                    tunnel.id,
                    "host_key",
                    "Host key confirmation required".into(),
                    Some(*challenge),
                );
                return;
            }
            Err((ConnectError::Verification(reason), _)) => {
                shared.emit(tunnel.id, "error", reason.clone(), None);
                shared.status(tunnel.id, "error", &reason, 0);
                return;
            }
            Err((error, was_running)) => (error.to_string(), was_running),
        };
        if was_running {
            attempt = 0;
            outage = Instant::now();
        }
        attempt = attempt.saturating_add(1);
        shared.emit(tunnel.id, "error", error.clone(), None);
        let Some(delay) = policy.delay(attempt, outage.elapsed()) else {
            shared.status(tunnel.id, "error", &error, attempt.saturating_sub(1));
            return;
        };
        shared.status(tunnel.id, "reconnecting", &error, attempt);
        if !reconnect::wait(delay, &cancel).await {
            shared.status(tunnel.id, "stopped", "", 0);
            return;
        }
        if matches!(policy, Policy::Legacy) && outage.elapsed() >= Duration::from_secs(900) {
            shared.status(tunnel.id, "error", &error, attempt.saturating_sub(1));
            return;
        }
    }
}

fn forwarding_start_error(tunnel: &Tunnel, hops: &[Hop], error: &std::io::Error) -> String {
    let remote = tunnel.mode == "remote";
    let (host, port) = if remote {
        (&tunnel.remote_host, tunnel.remote_port)
    } else {
        (&tunnel.local_host, tunnel.local_port)
    };
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let stage = if remote {
        "Remote port listener"
    } else {
        "Local port listener"
    };
    let jumper = hops
        .last()
        .map(|hop| {
            format!(
                " via jumper {} ({}:{})",
                hop.jumper.name, hop.jumper.host, hop.jumper.port
            )
        })
        .unwrap_or_default();
    let denied = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<russh::Error>())
        .is_some_and(|source| matches!(source, russh::Error::RequestDenied));
    let explanation = if remote && denied {
        "SSH authentication succeeded, but the server rejected the remote forwarding request. The server did not provide a specific reason. Check whether the remote port is occupied, the bind address is available, or server/account/key forwarding restrictions (AllowTcpForwarding, PermitListen, authorized_keys) deny it."
    } else if !remote && error.kind() == std::io::ErrorKind::AddrInUse {
        "The local listening address/port is already in use. Stop the conflicting listener or choose a different local port."
    } else if !remote && error.kind() == std::io::ErrorKind::PermissionDenied {
        "The operating system denied permission to bind the local listening address/port. Check port reservations and local security restrictions."
    } else if !remote && error.kind() == std::io::ErrorKind::AddrNotAvailable {
        "The local bind address is not available on this computer. Choose a local interface address."
    } else if error.kind() == std::io::ErrorKind::TimedOut {
        "The listening request timed out."
    } else {
        "Could not establish the listening endpoint."
    };
    format!("{stage} {endpoint}{jumper}: {explanation} Underlying error: {error}")
}

async fn run_once(
    tunnel: &Tunnel,
    hops: &[Hop],
    counters: Arc<Counters>,
    cancel: &CancellationToken,
    shared: &Shared,
) -> Result<(), (ConnectError, bool)> {
    let remote = tunnel.mode == "remote";
    let (chain, incoming) = if remote {
        let (chain, incoming) = connect_remote_chain(hops, cancel)
            .await
            .map_err(|error| (error, false))?;
        (chain, Some(incoming))
    } else {
        (
            connect_chain(hops, cancel)
                .await
                .map_err(|error| (error, false))?,
            None,
        )
    };
    let chain = Arc::new(chain);
    let child_cancel = cancel.child_token();
    let (errors, mut error_receiver) = mpsc::channel(128);
    let startup = async {
        if let Some(incoming) = incoming {
            let listener = RemoteListener::bind(&chain, tunnel, incoming).await?;
            Ok::<_, std::io::Error>(tokio::spawn(listener.serve(
                chain.clone(),
                tunnel.clone(),
                counters,
                child_cancel.clone(),
                errors,
            )))
        } else {
            let listener =
                TcpListener::bind((tunnel.local_host.as_str(), tunnel.local_port)).await?;
            Ok(tokio::spawn(serve_local(
                listener,
                tunnel.clone(),
                chain.clone(),
                counters,
                child_cancel.clone(),
                errors,
            )))
        }
    };
    let started = tokio::select! { biased; _ = cancel.cancelled() => Err(std::io::Error::other("Cancelled")), result = startup => result };
    let mut task = match started {
        Ok(task) => task,
        Err(error) => {
            chain.close().await;
            let context = forwarding_start_error(tunnel, hops, &error);
            return Err((ConnectError::Forwarding(context), false));
        }
    };
    shared.status(tunnel.id, "running", "", 0);
    let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
    let mut completed = false;
    let result = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Ok(()),
            outcome = &mut task => {
                completed = true;
                break match outcome {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(error)) => Err((ConnectError::Forwarding(format!("Forwarding worker stopped: {error}")), true)),
                    Err(_) => Err((ConnectError::Forwarding("Forwarding worker failed".into()), true)),
                };
            }
            Some(error) = error_receiver.recv() => { shared.emit(tunnel.id, "error", error, None); },
            _ = heartbeat.tick() => { if chain.is_closed() { break Ok(()); } }
        }
    };
    child_cancel.cancel();
    if !completed {
        let _ = task.await;
    }
    chain.close().await;
    result
}

#[cfg(test)]
mod journal_tests {
    use super::*;

    #[test]
    fn bind_error_context_preserves_the_cause_without_guessing_remote_rejections() {
        let mut tunnel = Tunnel {
            local_host: "::1".into(),
            local_port: 7897,
            ..Default::default()
        };
        for (kind, expected) in [
            (std::io::ErrorKind::PermissionDenied, "denied permission"),
            (
                std::io::ErrorKind::AddrNotAvailable,
                "not available on this computer",
            ),
        ] {
            let message =
                forwarding_start_error(&tunnel, &[], &std::io::Error::new(kind, "original cause"));
            assert!(message.contains("[::1]:7897"));
            assert!(message.contains(expected));
            assert!(message.contains("Underlying error: original cause"));
        }
        tunnel.mode = "remote".into();
        let message = forwarding_start_error(
            &tunnel,
            &[],
            &std::io::Error::new(std::io::ErrorKind::TimedOut, "Remote bind timed out"),
        );
        assert!(message.contains("request timed out"));
        assert!(!message.contains("server rejected"));
    }

    #[test]
    fn journal_is_timestamped_bounded_newest_first_and_clearable() {
        let runtime = Runtime::default();
        let mut events = runtime.subscribe();
        for index in 0..505 {
            runtime.activity(&format!("operation-{index}"), index % 2 == 0);
        }
        let journal = runtime.journal();
        assert_eq!(journal.len(), 500);
        assert_eq!(journal[0].message, "operation-504");
        assert_eq!(journal[499].message, "operation-5");
        assert_eq!(journal[0].kind, "action");
        assert_eq!(journal[1].kind, "action_error");
        assert!(journal.iter().all(|entry| entry.timestamp_ms > 0
            && entry.tunnel_id == 0
            && entry.challenge.is_none()));
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        assert!(events.try_recv().unwrap().timestamp_ms > 0);
        runtime.clear_journal();
        assert!(runtime.journal().is_empty());
    }
}
