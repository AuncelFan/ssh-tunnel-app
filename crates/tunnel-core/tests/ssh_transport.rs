use russh::{
    Channel, ChannelOpenFailure,
    keys::{Algorithm, PrivateKey},
    server,
};
use ssh_tunnel_core::{
    model::Jumper,
    ssh::{ConnectError, Hop, connect_chain},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

async fn unused_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn draft_probe_confirmation_saves_only_the_observed_key_and_retests() {
    use ssh_tunnel_core::{controller::Controller, probe_session::Request};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let known_hosts = dir.path().join("known_hosts");
    let controller = Controller::open(path, known_hosts.clone()).unwrap();
    let request = Request {
        jumpers: vec![hop(&fixture, 0).jumper],
        tunnel: None,
        diagnostics: true,
    };
    let token = controller.begin_probe(request.clone()).await.unwrap();
    let report = controller.run_probe(token).await.unwrap();
    assert!(report.challenge.is_some());
    assert!(controller.snapshot().trusted_hosts.is_empty());
    controller.approve_probe_key(token).await.unwrap();
    assert_eq!(controller.snapshot().trusted_hosts.len(), 1);
    assert!(controller.snapshot().jumpers.is_empty());
    assert!(controller.snapshot().tunnels.is_empty());
    assert!(!known_hosts.exists());
    assert!(controller.approve_probe_key(token).await.is_err());
    let report = controller.run_probe(token).await.unwrap();
    assert!(report.success, "{report:?}");
    assert!(controller.runtime_state().is_empty());
    fixture.assert_no_sessions().await;
}

#[tokio::test]
async fn probe_trust_rechecks_revocations_and_is_invalidated_by_config_switch() {
    use ssh_tunnel_core::{controller::Controller, probe_session::Request};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let controller = Controller::open(dir.path().join("config.toml"), known_hosts.clone()).unwrap();
    let request = Request {
        jumpers: vec![hop(&fixture, 0).jumper],
        tunnel: None,
        diagnostics: false,
    };
    let token = controller.begin_probe(request.clone()).await.unwrap();
    assert!(
        controller
            .run_probe(token)
            .await
            .unwrap()
            .challenge
            .is_some()
    );
    let revoked = format!(
        "@revoked [127.0.0.1]:{} {}\n",
        fixture.address.port(),
        fixture.key.public_key().to_openssh().unwrap()
    );
    std::fs::write(&known_hosts, &revoked).unwrap();
    assert!(controller.approve_probe_key(token).await.is_err());
    assert!(controller.snapshot().trusted_hosts.is_empty());
    assert_eq!(std::fs::read_to_string(&known_hosts).unwrap(), revoked);
    std::fs::write(&known_hosts, "").unwrap();
    controller
        .relocate_config(Some(dir.path().join("relocated")), true)
        .await
        .unwrap();
    assert!(controller.approve_probe_key(token).await.is_err());
    let token = controller.begin_probe(request.clone()).await.unwrap();
    assert!(
        controller
            .run_probe(token)
            .await
            .unwrap()
            .challenge
            .is_some()
    );
    controller.cancel_probe(token).await;
    assert!(controller.approve_probe_key(token).await.is_err());
    assert!(controller.snapshot().trusted_hosts.is_empty());
    let token = controller.begin_probe(request).await.unwrap();
    assert!(
        controller
            .run_probe(token)
            .await
            .unwrap()
            .challenge
            .is_some()
    );
    let preview = controller
        .preview_import(controller.config_path())
        .await
        .unwrap();
    controller.confirm_import(preview.token).await.unwrap();
    assert!(controller.approve_probe_key(token).await.is_err());
}

#[tokio::test]
async fn cancelling_native_probe_and_shutdown_wait_for_network_cleanup() {
    use ssh_tunnel_core::{controller::Controller, probe_session::Request};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let controller = Arc::new(
        Controller::open(
            dir.path().join("config.toml"),
            dir.path().join("known_hosts"),
        )
        .unwrap(),
    );
    for shutdown in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut jumper = hop(&fixture, 0).jumper;
        jumper.port = listener.local_addr().unwrap().port();
        jumper.timeout_ms = 30_000;
        let token = controller
            .begin_probe(Request {
                jumpers: vec![jumper],
                tunnel: None,
                diagnostics: false,
            })
            .await
            .unwrap();
        let running = controller.clone();
        let task = tokio::spawn(async move { running.run_probe(token).await.unwrap() });
        let (mut socket, _) = listener.accept().await.unwrap();
        if shutdown {
            controller.stop_all().await;
        } else {
            controller.cancel_probe(token).await;
        }
        let report = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(!report.success);
        assert_eq!(report.checks.last().unwrap().code, "cancelled");
        assert!(controller.approve_probe_key(token).await.is_err());
        let mut bytes = vec![];
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut bytes))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn connection_probes_cover_all_modes_and_release_local_and_remote_ports() {
    use ssh_tunnel_core::{model::Tunnel, probe::test_connection};
    let first = server().await;
    let last = server().await;
    let hops = [hop(&first, 1), hop(&last, 2)];
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for mode in ["local", "remote", "dynamic"] {
        let local_port = unused_port().await;
        let remote_port = if mode == "remote" {
            unused_port().await
        } else {
            target.local_addr().unwrap().port()
        };
        let tunnel = Tunnel {
            mode: mode.into(),
            local_port,
            remote_port,
            ..Default::default()
        };
        let report = test_connection(Some(&tunnel), &hops, &CancellationToken::new()).await;
        assert!(report.success, "{report:?}");
        assert!(
            report
                .latency_ms
                .is_some_and(|value| value.is_finite() && value >= 0.0)
        );
        assert!(report.challenge.is_none());
        let local = TcpListener::bind(("127.0.0.1", local_port)).await.unwrap();
        drop(local);
        if mode == "remote" {
            assert!(
                report
                    .checks
                    .iter()
                    .any(|check| check.phase == "remote_unbind" && check.ok)
            );
            let remote = TcpListener::bind(("127.0.0.1", remote_port)).await.unwrap();
            drop(remote);
        }
    }
    let jumper = test_connection(None, &hops[..1], &CancellationToken::new()).await;
    assert!(jumper.success, "{jumper:?}");
    first.assert_no_sessions().await;
    last.assert_no_sessions().await;
}

#[tokio::test]
async fn dynamic_probe_distinguishes_denied_forwarding_from_closed_target() {
    use ssh_tunnel_core::{model::Tunnel, probe::test_connection};
    let fixture = server_with_forwarding(true).await;
    let tunnel = Tunnel {
        mode: "dynamic".into(),
        local_port: unused_port().await,
        ..Default::default()
    };
    let report = test_connection(
        Some(&tunnel),
        &[hop(&fixture, 1)],
        &CancellationToken::new(),
    )
    .await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().code, "forwarding_denied");
    assert_eq!(report.checks.last().unwrap().phase, "dynamic_forwarding");
    fixture.assert_no_sessions().await;
}

#[tokio::test]
async fn diagnostics_separate_dns_tcp_key_and_ssh_without_running_commands() {
    use ssh_tunnel_core::probe::diagnose_connection;
    let fixture = server().await;
    let mut candidate = hop(&fixture, 1);
    let report = diagnose_connection(None, &[candidate.clone()], &CancellationToken::new()).await;
    assert!(report.success, "{report:?}");
    for phase in [
        "dns_first_hop",
        "tcp_first_hop",
        "ssh_authentication",
        "latency",
    ] {
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.phase == phase && check.ok)
        );
    }
    fixture.assert_no_sessions().await;
    candidate.jumper.auth_type = "ssh_key".into();
    let dir = tempfile::tempdir().unwrap();
    candidate.jumper.key_path = dir
        .path()
        .join("missing-key")
        .to_string_lossy()
        .into_owned();
    let report = diagnose_connection(None, &[candidate.clone()], &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "private_key_file");
    candidate.jumper.port = unused_port().await;
    let report = diagnose_connection(None, &[candidate.clone()], &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "tcp_first_hop");
    candidate.jumper.host = "invalid host name with spaces".into();
    let report = diagnose_connection(None, &[candidate], &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "dns_first_hop");
}

#[tokio::test]
async fn probe_bind_target_and_forwarding_failures_are_not_false_successes() {
    use ssh_tunnel_core::{model::Tunnel, probe::test_connection};
    let fixture = server().await;
    let hops = [hop(&fixture, 1)];
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut tunnel = Tunnel {
        local_port: occupied.local_addr().unwrap().port(),
        remote_port: unused_port().await,
        ..Default::default()
    };
    let report = test_connection(Some(&tunnel), &hops, &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "local_bind");
    tunnel.local_port = unused_port().await;
    let report = test_connection(Some(&tunnel), &hops, &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "remote_target");
    tunnel.remote_host = "denied.invalid".into();
    let report = test_connection(Some(&tunnel), &hops, &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "remote_target");
    tunnel.mode = "remote".into();
    tunnel.remote_host = "127.0.0.1".into();
    tunnel.remote_port = occupied.local_addr().unwrap().port();
    let report = test_connection(Some(&tunnel), &hops, &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().phase, "remote_bind");
    fixture.assert_no_sessions().await;
}

#[tokio::test]
async fn probe_authentication_and_host_trust_are_structured_and_do_not_expose_secrets() {
    use ssh_tunnel_core::probe::test_connection;
    let fixture = server().await;
    let mut candidate = hop(&fixture, 1);
    candidate.jumper.password = "sensitive-failed-auth-probe".into();
    let report = test_connection(None, &[candidate.clone()], &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().code, "authentication");
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains(&candidate.jumper.password)
    );
    candidate.jumper.password = "test-only-password".into();
    candidate.trusted_key = None;
    let report = test_connection(None, &[candidate], &CancellationToken::new()).await;
    assert!(!report.success);
    assert_eq!(report.checks.last().unwrap().code, "host_key");
    assert_eq!(report.challenge.unwrap().port, fixture.address.port());
    fixture.assert_no_sessions().await;
}

#[tokio::test]
async fn probe_timeout_and_cancellation_close_stalled_handshakes() {
    use ssh_tunnel_core::probe::test_connection;
    let fixture = server().await;
    for cancelled in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut candidate = hop(&fixture, 1);
        candidate.jumper.port = listener.local_addr().unwrap().port();
        candidate.jumper.timeout_ms = if cancelled { 30_000 } else { 80 };
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move { test_connection(None, &[candidate], &token).await });
        let (mut socket, _) = listener.accept().await.unwrap();
        if cancelled {
            cancel.cancel();
        }
        let report = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(!report.success);
        assert_eq!(
            report.checks.last().unwrap().code,
            if cancelled { "cancelled" } else { "timeout" }
        );
        // The client may have sent its banner; EOF must follow even without a server reply.
        let mut bytes = vec![];
        let result = tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut bytes))
            .await
            .unwrap();
        assert!(
            result.is_ok() || result.unwrap_err().kind() == std::io::ErrorKind::ConnectionReset
        );
    }
}

struct Server {
    active: Arc<std::sync::atomic::AtomicUsize>,
    address: std::net::SocketAddr,
    key: PrivateKey,
    client_key: PrivateKey,
    task: JoinHandle<()>,
    connections: Arc<std::sync::Mutex<CancellationToken>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        self.connections.lock().unwrap().cancel();
    }
}
impl Server {
    async fn assert_no_sessions(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.active.load(std::sync::atomic::Ordering::SeqCst) != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Probe left a live SSH session");
    }
    fn disconnect_clients(&self) {
        let mut cancel = self.connections.lock().unwrap();
        cancel.cancel();
        *cancel = CancellationToken::new();
    }
}

struct Fixture {
    deny_forwarding: bool,
    allowed_key: russh::keys::PublicKey,
    forwards: std::collections::HashMap<u32, JoinHandle<()>>,
}
impl Clone for Fixture {
    fn clone(&self) -> Self {
        Self {
            deny_forwarding: self.deny_forwarding,
            allowed_key: self.allowed_key.clone(),
            forwards: Default::default(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in self.forwards.values() {
            task.abort();
        }
    }
}
impl server::Handler for Fixture {
    type Error = russh::Error;
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        if self.deny_forwarding || address != "127.0.0.1" || *port > 65535 {
            return Ok(false);
        }
        let listener = match TcpListener::bind((address, *port as u16)).await {
            Ok(listener) => listener,
            Err(_) => return Ok(false),
        };
        *port = listener.local_addr()?.port().into();
        let allocated = *port;
        let handle = session.handle();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(_) = connections.join_next(), if !connections.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((mut socket, origin)) = accepted else { break };
                        let handle = handle.clone();
                        connections.spawn(async move {
                            if let Ok(channel) = handle.channel_open_forwarded_tcpip("127.0.0.1", allocated, origin.ip().to_string(), origin.port().into()).await {
                                let _ = tokio::io::copy_bidirectional(&mut socket, &mut channel.into_stream()).await;
                            }
                        });
                    }
                }
            }
        });
        self.forwards.insert(*port, task);
        Ok(true)
    }
    async fn cancel_tcpip_forward(
        &mut self,
        _: &str,
        port: u32,
        _: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        if let Some(task) = self.forwards.remove(&port) {
            task.abort();
            let _ = task.await;
            return Ok(true);
        }
        Ok(false)
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if user == "fixture" && password == "test-only-password" {
            server::Auth::Accept
        } else {
            server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            }
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(
            if user == "fixture" && key.key_data() == self.allowed_key.key_data() {
                server::Auth::Accept
            } else {
                server::Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                }
            },
        )
    }
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        // Test fixture deliberately never forwards outside loopback.
        if self.deny_forwarding || host != "127.0.0.1" || port > 65535 {
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        match TcpStream::connect((host, port as u16)).await {
            Ok(mut socket) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut channel.into_stream())
                        .await;
                });
            }
            Err(_) => {
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
            }
        }
        Ok(())
    }
}

async fn server() -> Server {
    server_with_forwarding(false).await
}

struct ActiveSession(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for ActiveSession {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn server_with_forwarding(deny_forwarding: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let client_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let handler = Fixture {
        deny_forwarding,
        allowed_key: client_key.public_key().clone(),
        forwards: Default::default(),
    };
    let config = Arc::new(server::Config {
        keys: vec![key.clone()],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let connections = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
    let connections_for_task = connections.clone();
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let active_for_task = active.clone();
    let task = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            let handler = handler.clone();
            let cancel = connections_for_task.lock().unwrap().clone();
            active_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let active = ActiveSession(active_for_task.clone());
            tokio::spawn(async move {
                let _active = active;
                let result = tokio::select! { _ = cancel.cancelled() => return, result = server::run_stream(config, socket, handler) => result };
                if let Ok(mut session) = result {
                    let handle = session.handle();
                    tokio::select! {
                        _ = &mut session => {},
                        _ = cancel.cancelled() => {
                            let _ = handle.disconnect(russh::Disconnect::ByApplication, "fixture restart".into(), "en".into()).await;
                            let _ = tokio::time::timeout(Duration::from_secs(1), session).await;
                        }
                    }
                }
            });
        }
    });
    Server {
        active,
        address,
        key,
        client_key,
        task,
        connections,
    }
}

fn hop(server: &Server, id: u64) -> Hop {
    Hop {
        jumper: Jumper {
            id,
            host: "127.0.0.1".into(),
            port: server.address.port(),
            user: "fixture".into(),
            auth_type: "password".into(),
            password: "test-only-password".into(),
            timeout_ms: 3000,
            ..Default::default()
        },
        trusted_key: Some(server.key.public_key().clone()),
        known_keys: Default::default(),
    }
}

#[tokio::test]
async fn password_auth_and_two_hop_forward_preserve_bytes() {
    let first = server().await;
    let second = server().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let (mut reader, mut writer) = stream.split();
        tokio::io::copy(&mut reader, &mut writer).await.unwrap();
    });
    let chain = connect_chain(
        &[hop(&first, 1), hop(&second, 2)],
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut channel = chain
        .last()
        .channel_open_direct_tcpip("127.0.0.1", address.port().into(), "127.0.0.1", 0)
        .await
        .unwrap()
        .into_stream();
    channel.write_all(b"two-hop payload").await.unwrap();
    let mut bytes = [0; 15];
    tokio::time::timeout(Duration::from_secs(3), channel.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"two-hop payload");
    channel.shutdown().await.unwrap();
    chain.close().await;
    echo.abort();
}

#[tokio::test]
async fn unknown_and_changed_keys_produce_endpoint_specific_challenges() {
    let fixture = server().await;
    for changed in [false, true] {
        let mut item = hop(&fixture, 17);
        item.trusted_key = changed.then(|| {
            PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
                .unwrap()
                .public_key()
                .clone()
        });
        let result = connect_chain(&[item], &CancellationToken::new()).await;
        let Err(ConnectError::HostKey(challenge)) = result else {
            panic!("Expected host-key challenge")
        };
        assert_eq!(challenge.jumper_id, 17);
        assert_eq!(challenge.port, fixture.address.port());
        assert_eq!(challenge.previous_fingerprint.is_some(), changed);
        assert_eq!(
            challenge.public_key,
            fixture.key.public_key().to_openssh().unwrap()
        );
    }
}

#[tokio::test]
async fn bad_password_and_cancellation_fail_without_bypass() {
    let fixture = server().await;
    let mut item = hop(&fixture, 1);
    item.jumper.password = "wrong".into();
    assert!(matches!(
        connect_chain(&[item], &CancellationToken::new()).await,
        Err(ConnectError::Authentication)
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        connect_chain(&[hop(&fixture, 1)], &cancel).await,
        Err(ConnectError::Cancelled)
    ));
}

#[tokio::test]
async fn private_key_authentication_works() {
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    let key = fixture.client_key.clone();
    key.write_openssh_file(&path, russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    let mut item = hop(&fixture, 1);
    item.jumper.auth_type = "ssh_key".into();
    item.jumper.key_path = path.to_string_lossy().into();
    item.jumper.password.clear();
    let chain = connect_chain(&[item], &CancellationToken::new())
        .await
        .unwrap();
    chain.close().await;
}

#[tokio::test]
async fn encrypted_private_key_requires_correct_passphrase() {
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("encrypted_ed25519");
    let encrypted = fixture
        .client_key
        .encrypt(&mut rand::rng(), "fixture-passphrase")
        .unwrap();
    encrypted
        .write_openssh_file(&path, russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    let mut item = hop(&fixture, 1);
    item.jumper.auth_type = "ssh_key".into();
    item.jumper.key_path = path.to_string_lossy().into();
    item.jumper.password = "fixture-passphrase".into();
    item.jumper.timeout_ms = 10000;
    let chain = connect_chain(&[item], &CancellationToken::new())
        .await
        .unwrap();
    chain.close().await;
    let mut wrong = hop(&fixture, 1);
    wrong.jumper.auth_type = "ssh_key".into();
    wrong.jumper.key_path = path.to_string_lossy().into();
    wrong.jumper.password = "wrong-passphrase".into();
    wrong.jumper.timeout_ms = 10000;
    assert!(matches!(
        connect_chain(&[wrong], &CancellationToken::new()).await,
        Err(ConnectError::Key(_))
    ));
}

#[tokio::test]
async fn trusted_first_hop_does_not_bypass_unknown_second_hop() {
    let first = server().await;
    let second = server().await;
    let mut unknown = hop(&second, 22);
    unknown.trusted_key = None;
    let result = connect_chain(&[hop(&first, 11), unknown], &CancellationToken::new()).await;
    let Err(ConnectError::HostKey(challenge)) = result else {
        panic!("Expected second-hop challenge")
    };
    assert_eq!(challenge.jumper_id, 22);
    assert_eq!(challenge.port, second.address.port());
}

#[tokio::test]
async fn stalled_handshake_times_out_and_can_be_cancelled() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let jumper = || Hop {
        jumper: Jumper {
            host: "127.0.0.1".into(),
            port: address.port(),
            user: "fixture".into(),
            auth_type: "password".into(),
            timeout_ms: 60,
            ..Default::default()
        },
        trusted_key: None,
        known_keys: Default::default(),
    };
    assert!(matches!(
        connect_chain(&[jumper()], &CancellationToken::new()).await,
        Err(ConnectError::Timeout)
    ));
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    assert!(matches!(
        connect_chain(&[jumper()], &cancel).await,
        Err(ConnectError::Cancelled)
    ));
}

#[cfg(windows)]
#[tokio::test]
async fn windows_named_pipe_agent_authenticates_without_reading_private_key_files() {
    use russh::keys::agent::{client::AgentClient, server::serve};
    use tokio::net::windows::named_pipe::ServerOptions;
    let fixture = server().await;
    let name = format!(
        r"\\.\pipe\ssh-tunnel-test-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    );
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)
        .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let pipe_name = name.clone();
    let listener = tokio::spawn(async move {
        let mut pipe = first;
        loop {
            if pipe.connect().await.is_err() {
                break;
            }
            let next = ServerOptions::new().create(&pipe_name).unwrap();
            if sender.send(Ok(pipe)).await.is_err() {
                break;
            }
            pipe = next;
        }
    });
    let agent_server = tokio::spawn(serve(
        tokio_stream::wrappers::ReceiverStream::new(receiver),
        (),
    ));
    let mut admin = AgentClient::connect_named_pipe(&name).await.unwrap();
    admin.add_identity(&fixture.client_key, &[]).await.unwrap();
    let mut item = hop(&fixture, 1);
    item.jumper.auth_type = "ssh_agent".into();
    item.jumper.password.clear();
    item.jumper.agent_socket_path = name;
    let result = connect_chain(&[item], &CancellationToken::new()).await;
    // Stop fixture infrastructure even if authentication fails.
    listener.abort();
    agent_server.abort();
    let chain = result.unwrap();
    chain.close().await;
}

#[tokio::test]
async fn local_and_socks_listeners_forward_payload_count_traffic_and_release_ports() {
    use ssh_tunnel_core::{forward::serve_local, model::Tunnel, traffic::Counters};
    for mode in ["local", "dynamic"] {
        let fixture = server().await;
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination_port = destination.local_addr().unwrap().port();
        let echo = tokio::spawn(async move {
            let (mut socket, _) = destination.accept().await.unwrap();
            let (mut reader, mut writer) = socket.split();
            let _ = tokio::io::copy(&mut reader, &mut writer).await;
        });
        let chain = Arc::new(
            connect_chain(&[hop(&fixture, 1)], &CancellationToken::new())
                .await
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let tunnel = Tunnel {
            mode: mode.into(),
            remote_port: destination_port,
            ..Default::default()
        };
        let counters = Arc::new(Counters::default());
        let cancel = CancellationToken::new();
        let (errors, mut error_receiver) = tokio::sync::mpsc::channel(8);
        let task = tokio::spawn(serve_local(
            listener,
            tunnel,
            chain.clone(),
            counters.clone(),
            cancel.clone(),
            errors,
        ));
        let mut client = TcpStream::connect(address).await.unwrap();
        if mode == "dynamic" {
            client.write_all(&[5, 1, 0]).await.unwrap();
            let mut auth = [0; 2];
            client.read_exact(&mut auth).await.unwrap();
            assert_eq!(auth, [5, 0]);
            client.write_all(&[5, 1, 0, 1, 127, 0, 0, 1]).await.unwrap();
            client
                .write_all(&destination_port.to_be_bytes())
                .await
                .unwrap();
            let mut reply = [0; 10];
            client.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply[1], 0);
        }
        client.write_all(b"payload").await.unwrap();
        let mut response = [0; 7];
        tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"payload");
        assert_eq!(counters.snapshot().uploaded, 7);
        assert_eq!(counters.snapshot().downloaded, 7);
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(error_receiver.try_recv().is_err());
        let rebound = TcpListener::bind(address).await.unwrap();
        drop(rebound);
        chain.close().await;
        echo.abort();
    }
}

#[tokio::test]
async fn remote_forward_through_two_hops_releases_port_and_can_rebind() {
    use ssh_tunnel_core::{
        forward::RemoteListener, model::Tunnel, ssh::connect_remote_chain, traffic::Counters,
    };
    let first = server().await;
    let second = server().await;
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_port = destination.local_addr().unwrap().port();
    let echo = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            let (mut socket, _) = destination.accept().await.unwrap();
            tasks.spawn(async move {
                let (mut reader, mut writer) = socket.split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    let mut remote_port = 0;
    for _ in 0..2 {
        let cancel = CancellationToken::new();
        let (chain, incoming) = connect_remote_chain(&[hop(&first, 1), hop(&second, 2)], &cancel)
            .await
            .unwrap();
        let chain = Arc::new(chain);
        let tunnel = Tunnel {
            mode: "remote".into(),
            local_port,
            remote_port,
            ..Default::default()
        };
        let listener = RemoteListener::bind(&chain, &tunnel, incoming)
            .await
            .unwrap();
        remote_port = listener.port();
        let counters = Arc::new(Counters::default());
        let (errors, mut received) = tokio::sync::mpsc::channel(8);
        let task = tokio::spawn(listener.serve(
            chain.clone(),
            tunnel,
            counters.clone(),
            cancel.clone(),
            errors,
        ));
        let mut client = TcpStream::connect(("127.0.0.1", remote_port))
            .await
            .unwrap();
        client.write_all(b"reverse payload").await.unwrap();
        let mut response = [0; 15];
        tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"reverse payload");
        assert_eq!(counters.snapshot().uploaded, 15);
        assert_eq!(counters.snapshot().downloaded, 15);
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(4), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(received.try_recv().is_err());
        let rebound = TcpListener::bind(("127.0.0.1", remote_port)).await.unwrap();
        drop(rebound);
        chain.close().await;
    }
    echo.abort();
}

async fn await_runtime_status(
    events: &mut tokio::sync::broadcast::Receiver<ssh_tunnel_core::runtime::RuntimeEvent>,
    expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.kind == "status" && event.message == expected {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn runtime_stop_interrupts_retry_wait_and_finite_budget_expires() {
    use ssh_tunnel_core::{model::Tunnel, runtime::Runtime};
    let fixture = server().await;
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = occupied.local_addr().unwrap().port();
    let runtime = Runtime::default();
    let mut events = runtime.subscribe();
    let mut tunnel = Tunnel {
        id: 1,
        name: "Retries".into(),
        local_port: port,
        remote_port: 80,
        reconnect_mode: "fixed".into(),
        reconnect_interval_seconds: 60,
        ..Default::default()
    };
    runtime
        .start(tunnel.clone(), vec![hop(&fixture, 1)])
        .await
        .unwrap();
    await_runtime_status(&mut events, "reconnecting").await;
    tokio::time::timeout(Duration::from_millis(500), runtime.stop(1))
        .await
        .unwrap();
    assert_eq!(runtime.snapshots()[0].status, "stopped");
    tunnel.reconnect_interval_seconds = 1;
    tunnel.reconnect_max_attempts = 1;
    let before = runtime.logs(1).len();
    runtime.start(tunnel, vec![hop(&fixture, 1)]).await.unwrap();
    await_runtime_status(&mut events, "error").await;
    let snapshot = &runtime.snapshots()[0];
    assert_eq!(snapshot.status, "error");
    assert_eq!(snapshot.retry_attempt, 1);
    assert!(
        snapshot
            .last_error
            .contains(&format!("Local port listener 127.0.0.1:{port}"))
    );
    assert!(snapshot.last_error.contains("already in use"));
    assert!(
        !snapshot
            .last_error
            .contains("Invalid SSH connection parameters")
    );
    assert_eq!(
        runtime.logs(1)[before..]
            .iter()
            .filter(|event| event.kind == "status" && event.message == "connecting")
            .count(),
        2
    );
    runtime.stop_all().await;
}

#[tokio::test]
async fn runtime_remote_rejection_reports_endpoint_and_uncertainty_without_credentials() {
    use ssh_tunnel_core::{model::Tunnel, runtime::Runtime};
    let fixture = server().await;
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = occupied.local_addr().unwrap().port();
    let runtime = Runtime::default();
    let mut events = runtime.subscribe();
    let tunnel = Tunnel {
        id: 1,
        mode: "remote".into(),
        remote_port: port,
        local_port: 7897,
        reconnect_mode: "disabled".into(),
        ..Default::default()
    };
    let mut candidate = hop(&fixture, 1);
    candidate.jumper.name = "fixture-jumper".into();
    runtime.start(tunnel, vec![candidate]).await.unwrap();
    await_runtime_status(&mut events, "error").await;
    let message = runtime.snapshots()[0].last_error.clone();
    assert!(message.contains(&format!("Remote port listener 127.0.0.1:{port}")));
    assert!(message.contains("fixture-jumper"));
    assert!(message.contains("SSH authentication succeeded"));
    assert!(message.contains("did not provide a specific reason"));
    assert!(message.contains("Underlying error: The request was rejected by the other party"));
    assert!(!message.contains("Invalid SSH connection parameters"));
    assert!(
        runtime
            .logs(1)
            .iter()
            .any(|event| event.kind == "error" && event.message == message)
    );
    runtime.stop_all().await;
}

#[tokio::test]
async fn runtime_host_key_challenge_is_terminal_until_explicit_resolution() {
    use ssh_tunnel_core::{model::Tunnel, runtime::Runtime};
    let fixture = server().await;
    let mut unknown = hop(&fixture, 1);
    unknown.trusted_key = None;
    let runtime = Runtime::default();
    let mut events = runtime.subscribe();
    let tunnel = Tunnel {
        id: 42,
        name: "Trust".into(),
        local_port: 18080,
        remote_port: 80,
        reconnect_mode: "fixed".into(),
        reconnect_interval_seconds: 1,
        ..Default::default()
    };
    runtime.start(tunnel, vec![unknown]).await.unwrap();
    await_runtime_status(&mut events, "awaiting_trust").await;
    let event = events.recv().await.unwrap();
    assert_eq!(event.kind, "host_key");
    assert_eq!(event.challenge.unwrap().port, fixture.address.port());
    assert_eq!(runtime.snapshots()[0].status, "awaiting_trust");
    assert!(
        tokio::time::timeout(Duration::from_millis(1100), events.recv())
            .await
            .is_err()
    );
    runtime.stop(42).await;
}

#[tokio::test]
async fn runtime_serializes_start_stop_and_releases_local_and_remote_listeners() {
    use ssh_tunnel_core::{model::Tunnel, runtime::Runtime};
    let fixture = server().await;
    for mode in ["local", "remote"] {
        let reserve = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let tunnel = Tunnel {
            id: 7,
            name: "Lifecycle".into(),
            mode: mode.into(),
            local_port: port,
            remote_port: port,
            reconnect_mode: "disabled".into(),
            ..Default::default()
        };
        let runtime = Runtime::default();
        let mut events = runtime.subscribe();
        for _ in 0..2 {
            runtime
                .start(tunnel.clone(), vec![hop(&fixture, 1)])
                .await
                .unwrap();
            await_runtime_status(&mut events, "running").await;
            assert!(
                runtime
                    .start(tunnel.clone(), vec![hop(&fixture, 1)])
                    .await
                    .is_err()
            );
            runtime.stop(7).await;
            assert_eq!(runtime.snapshots()[0].status, "stopped");
            let rebound = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
            drop(rebound);
        }
    }
}

#[tokio::test]
async fn runtime_reconnects_after_ssh_disconnect_and_rebinds_remote_port() {
    use ssh_tunnel_core::{model::Tunnel, runtime::Runtime};
    for mode in ["local", "remote", "dynamic"] {
        let fixture = server().await;
        let reserve = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let runtime = Runtime::default();
        let mut events = runtime.subscribe();
        let tunnel = Tunnel {
            id: 8,
            name: "Recover".into(),
            mode: mode.into(),
            local_port: port,
            remote_port: port,
            reconnect_mode: "fixed".into(),
            reconnect_interval_seconds: 1,
            reconnect_max_attempts: 2,
            ..Default::default()
        };
        runtime.start(tunnel, vec![hop(&fixture, 1)]).await.unwrap();
        await_runtime_status(&mut events, "running").await;
        fixture.disconnect_clients();
        await_runtime_status(&mut events, "reconnecting").await;
        await_runtime_status(&mut events, "running").await;
        assert_eq!(runtime.snapshots()[0].retry_attempt, 0);
        runtime.stop_all().await;
        let rebound = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        drop(rebound);
    }
}

#[tokio::test]
async fn controller_trusts_or_replaces_only_confirmed_key_and_persists_without_bypass() {
    use ssh_tunnel_core::{controller::Controller, model::Tunnel};
    for mismatch in [false, true] {
        let fixture = server().await;
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let known_hosts = dir.path().join("known_hosts");
        let system_text = if mismatch {
            format!(
                "[127.0.0.1]:{} {}\n",
                fixture.address.port(),
                PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
                    .unwrap()
                    .public_key()
                    .to_openssh()
                    .unwrap()
            )
        } else {
            String::new()
        };
        std::fs::write(&known_hosts, &system_text).unwrap();
        let controller = Controller::open(config.clone(), known_hosts.clone()).unwrap();
        let mut events = controller.subscribe();
        let mut jumper = hop(&fixture, 0).jumper;
        jumper.name = "Fixture".into();
        let id = controller.save_jumper(jumper).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let tunnel = controller
            .save_tunnel(Tunnel {
                name: "Trust fixture".into(),
                jumper_ids: vec![id],
                local_port: port,
                remote_port: 80,
                ..Default::default()
            })
            .await
            .unwrap();
        controller.start(tunnel).await.unwrap();
        await_runtime_status(&mut events, "awaiting_trust").await;
        let challenge = events.recv().await.unwrap();
        assert_eq!(
            challenge
                .challenge
                .as_ref()
                .unwrap()
                .previous_fingerprint
                .is_some(),
            mismatch
        );
        assert!(
            controller
                .resolve_host_key(tunnel, challenge.generation + 1, true)
                .await
                .is_err()
        );
        controller
            .resolve_host_key(tunnel, challenge.generation, true)
            .await
            .unwrap();
        await_runtime_status(&mut events, "running").await;
        assert!(!controller.snapshot().jumpers[0].bypass_host_verification);
        assert_eq!(controller.snapshot().trusted_hosts.len(), 1);
        assert!(
            controller
                .resolve_host_key(tunnel, challenge.generation, true)
                .await
                .is_err()
        );
        controller.stop_all().await;
        assert_eq!(std::fs::read_to_string(&known_hosts).unwrap(), system_text);
        let reloaded = Controller::open(config, known_hosts).unwrap();
        let mut events = reloaded.subscribe();
        reloaded.start(tunnel).await.unwrap();
        await_runtime_status(&mut events, "running").await;
        assert!(reloaded.pending_trust().is_empty());
        reloaded.stop_all().await;
    }
}

#[tokio::test]
async fn configuration_import_is_confirmed_snapshotted_backed_up_and_preserves_running_on_failure()
{
    use ssh_tunnel_core::{controller::Controller, model::Tunnel, storage::Storage};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let source = dir.path().join("import.toml");
    let known_hosts = dir.path().join("known_hosts");
    std::fs::write(
        &known_hosts,
        format!(
            "[127.0.0.1]:{} {}\n",
            fixture.address.port(),
            fixture.key.public_key().to_openssh().unwrap()
        ),
    )
    .unwrap();
    let controller = Controller::open(path.clone(), known_hosts).unwrap();
    let jumper = controller
        .save_jumper(hop(&fixture, 0).jumper)
        .await
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let id = controller
        .save_tunnel(Tunnel {
            name: "Original".into(),
            jumper_ids: vec![jumper],
            local_port: port,
            remote_port: 80,
            ..Default::default()
        })
        .await
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut events = controller.subscribe();
    controller.start(id).await.unwrap();
    await_runtime_status(&mut events, "running").await;
    let generation = controller.runtime_state()[0].generation;
    assert!(
        controller
            .preview_import(dir.path().join("missing.toml"))
            .await
            .is_err()
    );
    assert!(controller.export_config(path.clone()).await.is_err());
    controller.export_config(source.clone()).await.unwrap();
    let mut imported = Storage::read_import(&source).unwrap();
    assert_eq!(imported.tunnels[0].status, "stopped");
    imported.tunnels[0].name = "Imported".into();
    imported.tunnels[0].auto_start = true;
    Storage::new(&source).save(&imported).unwrap();
    let cancelled = controller.preview_import(source.clone()).await.unwrap();
    controller.cancel_import().await;
    assert!(controller.confirm_import(cancelled.token).await.is_err());
    assert_eq!(controller.runtime_state()[0].generation, generation);
    let first = controller.preview_import(source.clone()).await.unwrap();
    let preview = controller.preview_import(source.clone()).await.unwrap();
    assert_eq!(preview.auto_start, 1);
    assert_eq!(preview.tunnels, 1);
    assert!(controller.confirm_import(first.token).await.is_err());
    // Confirmation applies the exact validated preview, not a later changed file.
    std::fs::write(&source, "invalid [ TOML secret-not-in-error").unwrap();
    assert!(
        controller
            .confirm_import(preview.token)
            .await
            .unwrap()
            .is_empty()
    );
    await_runtime_status(&mut events, "running").await;
    assert_eq!(controller.snapshot().tunnels[0].name, "Imported");
    assert!(controller.runtime_state()[0].generation > generation);
    assert!(
        controller
            .logs(id)
            .iter()
            .all(|event| event.generation > generation)
    );
    assert!(controller.confirm_import(preview.token).await.is_err());
    assert!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("config-backup-"))
            .any(|entry| std::fs::read(entry.path()).unwrap() == original)
    );
    let error = controller
        .preview_import(source.clone())
        .await
        .err()
        .unwrap();
    assert!(!error.contains("secret-not-in-error"));
    Storage::new(&source).save(&imported).unwrap();
    let preview = controller.preview_import(source).await.unwrap();
    std::fs::rename(&path, dir.path().join("preserved.toml")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(controller.confirm_import(preview.token).await.is_err());
    assert_eq!(controller.snapshot().tunnels[0].status, "running");
    assert!(TcpListener::bind(("127.0.0.1", port)).await.is_err());
    controller.stop_all().await;
}

#[tokio::test]
async fn startup_launches_only_marked_tunnels_and_shutdown_releases_listeners() {
    use ssh_tunnel_core::{controller::Controller, model::Tunnel};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let known_hosts = dir.path().join("known_hosts");
    std::fs::write(
        &known_hosts,
        format!(
            "[127.0.0.1]:{} {}\n",
            fixture.address.port(),
            fixture.key.public_key().to_openssh().unwrap()
        ),
    )
    .unwrap();
    let controller = Controller::open(path.clone(), known_hosts.clone()).unwrap();
    let jumper = controller
        .save_jumper(hop(&fixture, 0).jumper)
        .await
        .unwrap();
    let mut reservations = Vec::new();
    let mut ports = Vec::new();
    for index in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        ports.push(port);
        reservations.push(listener);
        controller
            .save_tunnel(Tunnel {
                name: format!("Startup {index}"),
                jumper_ids: vec![jumper],
                local_port: port,
                remote_port: 80,
                auto_start: index != 1,
                ..Default::default()
            })
            .await
            .unwrap();
    }
    drop(controller);
    drop(reservations);
    let controller = Controller::open(path, known_hosts).unwrap();
    let mut events = controller.subscribe();
    assert!(controller.start_automatically().await.is_empty());
    for _ in 0..2 {
        await_runtime_status(&mut events, "running").await;
    }
    let snapshot = controller.snapshot();
    assert_eq!(
        snapshot
            .tunnels
            .iter()
            .map(|t| t.status.as_str())
            .collect::<Vec<_>>(),
        vec!["running", "stopped", "running"]
    );
    assert!(TcpListener::bind(("127.0.0.1", ports[0])).await.is_err());
    assert!(TcpListener::bind(("127.0.0.1", ports[1])).await.is_ok());
    assert!(TcpListener::bind(("127.0.0.1", ports[2])).await.is_err());
    controller.stop_all().await;
    for port in ports {
        assert!(TcpListener::bind(("127.0.0.1", port)).await.is_ok());
    }
}

#[tokio::test]
async fn controller_cancelled_trust_does_not_persist_and_revocation_blocks_approval() {
    use ssh_tunnel_core::{controller::Controller, model::Tunnel};
    let fixture = server().await;
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let controller = Controller::open(dir.path().join("config.toml"), known_hosts.clone()).unwrap();
    let mut events = controller.subscribe();
    let id = controller
        .save_jumper(hop(&fixture, 0).jumper)
        .await
        .unwrap();
    let tunnel = controller
        .save_tunnel(Tunnel {
            name: "Cancel trust".into(),
            jumper_ids: vec![id],
            local_port: 18080,
            remote_port: 80,
            ..Default::default()
        })
        .await
        .unwrap();
    controller.start(tunnel).await.unwrap();
    await_runtime_status(&mut events, "awaiting_trust").await;
    let first = events.recv().await.unwrap();
    controller
        .resolve_host_key(tunnel, first.generation, false)
        .await
        .unwrap();
    assert!(controller.snapshot().trusted_hosts.is_empty());
    assert!(controller.pending_trust().is_empty());
    controller.start(tunnel).await.unwrap();
    await_runtime_status(&mut events, "awaiting_trust").await;
    let second = events.recv().await.unwrap();
    assert_ne!(first.generation, second.generation);
    assert!(
        controller
            .resolve_host_key(tunnel, first.generation, true)
            .await
            .is_err()
    );
    std::fs::write(
        &known_hosts,
        format!(
            "@revoked [127.0.0.1]:{} {}",
            fixture.address.port(),
            fixture.key.public_key().to_openssh().unwrap()
        ),
    )
    .unwrap();
    assert!(
        controller
            .resolve_host_key(tunnel, second.generation, true)
            .await
            .is_err()
    );
    assert!(controller.snapshot().trusted_hosts.is_empty());
    controller.stop_all().await;
}
