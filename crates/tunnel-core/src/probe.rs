use crate::{
    model::Tunnel,
    ssh::{self, ConnectError, Hop, HostKeyChallenge},
};
use serde::Serialize;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::net::{TcpListener, TcpStream, lookup_host};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Serialize)]
pub struct Check {
    pub phase: &'static str,
    pub ok: bool,
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub success: bool,
    pub elapsed_ms: u64,
    pub latency_ms: Option<f64>,
    pub checks: Vec<Check>,
    pub challenge: Option<HostKeyChallenge>,
}

impl Report {
    fn passed(&mut self, phase: &'static str) {
        self.checks.push(Check {
            phase,
            ok: true,
            code: "ok",
            detail: String::new(),
        });
    }
    fn failed(&mut self, phase: &'static str, code: &'static str, detail: impl Into<String>) {
        self.checks.push(Check {
            phase,
            ok: false,
            code,
            detail: detail.into(),
        });
    }
    fn ssh_failure(&mut self, phase: &'static str, error: ConnectError) {
        let code = match &error {
            ConnectError::HostKey(_) => "host_key",
            ConnectError::Cancelled => "cancelled",
            ConnectError::Timeout => "timeout",
            ConnectError::Authentication => "authentication",
            ConnectError::Invalid(_) => "invalid",
            ConnectError::Forwarding(_) => "forwarding",
            ConnectError::Verification(_) => "verification",
            ConnectError::Key(_) => "key",
            ConnectError::Agent(_) => "agent",
            ConnectError::Transport(russh::Error::ChannelOpenFailure(
                russh::ChannelOpenFailure::AdministrativelyProhibited,
            )) => "forwarding_denied",
            ConnectError::Transport(russh::Error::IO(error))
                if error.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                "refused"
            }
            ConnectError::Transport(_) => "transport",
        };
        self.failed(phase, code, error.to_string());
        if let ConnectError::HostKey(challenge) = error {
            self.challenge = Some(*challenge);
        }
    }
}

async fn bounded<T>(
    cancel: &CancellationToken,
    timeout: Duration,
    operation: impl Future<Output = Result<T, ConnectError>>,
) -> Result<T, ConnectError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ConnectError::Cancelled),
        result = tokio::time::timeout(timeout, operation) => result.unwrap_or(Err(ConnectError::Timeout)),
    }
}

/// Test a draft without saving it or changing host trust. Every SSH session is closed
/// before returning, including failed/ cancelled forwarding requests.
pub async fn test_connection(
    tunnel: Option<&Tunnel>,
    hops: &[Hop],
    cancel: &CancellationToken,
) -> Report {
    probe(tunnel, hops, cancel, false).await
}

/// Local diagnostics use the same SSH engine, never a shell or a cloud service.
/// Only the first hop is resolved locally; subsequent hops may be private SSH-side names.
pub async fn diagnose_connection(
    tunnel: Option<&Tunnel>,
    hops: &[Hop],
    cancel: &CancellationToken,
) -> Report {
    probe(tunnel, hops, cancel, true).await
}

async fn probe(
    tunnel: Option<&Tunnel>,
    hops: &[Hop],
    cancel: &CancellationToken,
    diagnostics: bool,
) -> Report {
    let started = Instant::now();
    let mut report = Report {
        success: false,
        elapsed_ms: 0,
        latency_ms: None,
        checks: vec![],
        challenge: None,
    };
    run(tunnel, hops, cancel, diagnostics, &mut report).await;
    report.elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    report.success = !report.checks.is_empty() && report.checks.iter().all(|check| check.ok);
    report
}

async fn run(
    tunnel: Option<&Tunnel>,
    hops: &[Hop],
    cancel: &CancellationToken,
    diagnostics: bool,
    report: &mut Report,
) {
    if cancel.is_cancelled() {
        report.ssh_failure("configuration", ConnectError::Cancelled);
        return;
    }
    if hops.is_empty()
        || hops.iter().any(|hop| {
            hop.jumper.host.trim().is_empty()
                || hop.jumper.user.trim().is_empty()
                || hop.jumper.port == 0
        })
    {
        report.failed(
            "configuration",
            "invalid",
            "A nonempty SSH chain with host, port and user is required",
        );
        return;
    }
    if let Some(tunnel) = tunnel
        && (!["local", "remote", "dynamic"].contains(&tunnel.mode.as_str())
            || tunnel.local_host.trim().is_empty()
            || tunnel.local_port == 0
            || (tunnel.mode != "dynamic"
                && (tunnel.remote_host.trim().is_empty() || tunnel.remote_port == 0)))
    {
        report.failed(
            "configuration",
            "invalid",
            "Invalid forwarding mode or endpoint",
        );
        return;
    }
    report.passed("configuration");
    if diagnostics && !preflight(hops, cancel, report).await {
        return;
    }
    let timeout = Duration::from_millis(match hops.last().unwrap().jumper.timeout_ms {
        0 => 5000,
        value => value,
    });
    if let Some(tunnel) = tunnel.filter(|t| t.mode != "remote") {
        let result = bounded(cancel, timeout, async {
            let listener = TcpListener::bind((tunnel.local_host.as_str(), tunnel.local_port))
                .await
                .map_err(russh::Error::from)?;
            drop(listener);
            Ok(())
        })
        .await;
        if let Err(error) = result {
            report.ssh_failure("local_bind", error);
            return;
        }
        report.passed("local_bind");
    }
    let chain = match ssh::connect_chain(hops, cancel).await {
        Ok(chain) => {
            report.passed("ssh_authentication");
            chain
        }
        Err(error) => {
            report.ssh_failure("ssh_authentication", error);
            return;
        }
    };
    let latency_start = Instant::now();
    let latency = bounded(cancel, timeout, async {
        // Failure to recognize keepalive still supplies a valid SSH round-trip reply.
        match chain
            .last()
            .send_global_request("keepalive@openssh.com", &[], true)
            .await
        {
            Ok(_) | Err(russh::Error::RequestDenied) => Ok(()),
            Err(error) => Err(error.into()),
        }
    })
    .await;
    match latency {
        Ok(()) => {
            report.latency_ms = Some(latency_start.elapsed().as_secs_f64() * 1000.0);
            report.passed("latency");
            if let Some(tunnel) = tunnel {
                forwarding(&chain, tunnel, cancel, timeout, report).await;
            }
        }
        Err(error) => report.ssh_failure("latency", error),
    }
    // Cleanup deliberately ignores user cancellation, so successful remote binds are revoked.
    chain.close().await;
}

async fn preflight(hops: &[Hop], cancel: &CancellationToken, report: &mut Report) -> bool {
    let first = &hops[0].jumper;
    let timeout = Duration::from_millis(if first.timeout_ms == 0 {
        5000
    } else {
        first.timeout_ms
    });
    let resolved = bounded(cancel, timeout, async {
        let addresses: Vec<_> = lookup_host((first.host.as_str(), first.port))
            .await
            .map_err(russh::Error::from)?
            .collect();
        if addresses.is_empty() {
            return Err(ConnectError::Invalid("DNS returned no addresses".into()));
        }
        Ok(addresses)
    })
    .await;
    let addresses = match resolved {
        Ok(addresses) => {
            report.passed("dns_first_hop");
            addresses
        }
        Err(error) => {
            report.ssh_failure("dns_first_hop", error);
            return false;
        }
    };
    let tcp = bounded(cancel, timeout, async {
        let socket = TcpStream::connect(addresses.as_slice())
            .await
            .map_err(russh::Error::from)?;
        drop(socket);
        Ok(())
    })
    .await;
    if let Err(error) = tcp {
        report.ssh_failure("tcp_first_hop", error);
        return false;
    }
    report.passed("tcp_first_hop");
    for hop in hops.iter().filter(|hop| hop.jumper.auth_type == "ssh_key") {
        let key = bounded(cancel, timeout, async {
            let path = ssh::expand_key_path(&hop.jumper.key_path)?;
            let file = tokio::fs::File::open(path)
                .await
                .map_err(russh::Error::from)?;
            if !file.metadata().await.map_err(russh::Error::from)?.is_file() {
                return Err(ConnectError::Invalid(
                    "Private key path is not a file".into(),
                ));
            }
            Ok(())
        })
        .await;
        if let Err(error) = key {
            report.ssh_failure("private_key_file", error);
            return false;
        }
        report.passed("private_key_file");
    }
    true
}

async fn forwarding(
    chain: &ssh::Chain,
    tunnel: &Tunnel,
    cancel: &CancellationToken,
    timeout: Duration,
    report: &mut Report,
) {
    if tunnel.mode == "remote" {
        let bound = bounded(cancel, timeout, async {
            Ok(chain
                .last()
                .tcpip_forward(&tunnel.remote_host, u32::from(tunnel.remote_port))
                .await?)
        })
        .await;
        match bound {
            Ok(_) => {
                report.passed("remote_bind");
                let cleanup = tokio::time::timeout(
                    timeout,
                    chain
                        .last()
                        .cancel_tcpip_forward(&tunnel.remote_host, u32::from(tunnel.remote_port)),
                )
                .await;
                match cleanup {
                    Ok(Ok(())) => report.passed("remote_unbind"),
                    Ok(Err(error)) => report.ssh_failure("remote_unbind", error.into()),
                    Err(_) => report.ssh_failure("remote_unbind", ConnectError::Timeout),
                }
            }
            Err(error) => report.ssh_failure("remote_bind", error),
        }
        return;
    }
    let dynamic = tunnel.mode == "dynamic";
    let host = if dynamic {
        "127.0.0.1"
    } else {
        tunnel.remote_host.as_str()
    };
    let port = if dynamic { 1 } else { tunnel.remote_port };
    let phase = if dynamic {
        "dynamic_forwarding"
    } else {
        "remote_target"
    };
    let result = bounded(cancel, timeout, async {
        match chain
            .last()
            .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
            .await
        {
            Ok(channel) => {
                channel.close().await?;
                Ok(())
            }
            Err(russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed))
                if dynamic =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    })
    .await;
    match result {
        Ok(()) => report.passed(phase),
        Err(error) => report.ssh_failure(phase, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_chain_is_rejected_before_any_network_checks() {
        let report = diagnose_connection(None, &[], &CancellationToken::new()).await;
        assert!(!report.success);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].code, "invalid");
        assert!(report.latency_ms.is_none());
    }

    #[tokio::test]
    async fn cancelled_request_never_starts_validation_or_network_checks() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let report = test_connection(None, &[], &cancel).await;
        assert!(!report.success);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].code, "cancelled");
    }
}
