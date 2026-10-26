use crate::hostkeys::{HostKeys, Verification};
use crate::model::Jumper;
use russh::{
    client,
    keys::{self, HashAlg, PublicKey, PublicKeyOrCertificate},
};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct IncomingForward {
    pub channel: russh::Channel<client::Msg>,
    pub address: String,
    pub port: u32,
    pub reply: client::ChannelOpenHandle,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostKeyChallenge {
    pub jumper_id: u64,
    pub host: String,
    pub port: u16,
    pub previous_fingerprint: Option<String>,
    pub fingerprint: String,
    pub public_key: String,
    pub authority_fingerprint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("Host key confirmation required")]
    HostKey(Box<HostKeyChallenge>),
    #[error("SSH operation cancelled")]
    Cancelled,
    #[error("SSH connection or authentication timed out")]
    Timeout,
    #[error("SSH authentication rejected")]
    Authentication,
    #[error("Invalid SSH connection parameters: {0}")]
    Invalid(String),
    #[error("Tunnel forwarding failed: {0}")]
    Forwarding(String),
    #[error("SSH transport: {0}")]
    Transport(#[from] russh::Error),
    #[error("SSH key or agent: {0}")]
    Key(#[from] keys::Error),
    #[error("SSH agent authentication: {0}")]
    Agent(String),
    #[error("Host key verification failed: {0}")]
    Verification(String),
}

#[derive(Clone)]
pub struct Hop {
    pub jumper: Jumper,
    /// Exact key approved for this endpoint; never shared implicitly with another hop.
    pub trusted_key: Option<PublicKey>,
    pub known_keys: HostKeys,
}

pub struct Handler {
    jumper_id: u64,
    host: String,
    port: u16,
    known_keys: HostKeys,
    bypass: bool,
    challenge: Arc<Mutex<Option<HostKeyChallenge>>>,
    forwarded: Option<mpsc::Sender<IncomingForward>>,
    denied: Arc<Mutex<Option<String>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        address: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: client::ChannelOpenHandle,
        _: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Some(sender) = &self.forwarded {
            // Dropping an unsolicited/overflowed request rejects it without blocking SSH.
            let _ = sender.try_send(IncomingForward {
                channel,
                address: address.into(),
                port,
                reply,
            });
        }
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        offered: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = offered.public_key();
        let known = self.known_keys.clone();
        let host = self.host.clone();
        let port = self.port;
        match tokio::task::spawn_blocking(move || known.refresh(&host, port)).await {
            Ok(Ok(keys)) => self.known_keys = keys,
            _ => {
                *self.denied.lock().expect("Host verification lock poisoned") =
                    Some("Cannot reload known_hosts safely".into());
                return Ok(false);
            }
        }
        match self.known_keys.verify(&self.host, offered) {
            Verification::Revoked => {
                *self.denied.lock().expect("Host verification lock poisoned") =
                    Some("Server key or certificate authority is revoked".into());
                return Ok(false);
            }
            Verification::Trusted => return Ok(true),
            _ if self.bypass => return Ok(true),
            Verification::InvalidCertificate => {
                *self.denied.lock().expect("Host verification lock poisoned") = Some(
                    "Server certificate is invalid, expired, or not valid for this host".into(),
                );
                return Ok(false);
            }
            Verification::Untrusted => {}
        }
        let challenge = HostKeyChallenge {
            jumper_id: self.jumper_id,
            host: self.host.clone(),
            port: self.port,
            previous_fingerprint: self
                .known_keys
                .trusted
                .iter()
                .find(|expected| expected.algorithm() == key.algorithm())
                .map(|k| k.fingerprint(HashAlg::Sha256).to_string()),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            public_key: key.to_openssh().map_err(keys::Error::from)?,
            authority_fingerprint: offered.certificate().map(|cert| {
                cert.signature_key()
                    .fingerprint(HashAlg::Sha256)
                    .to_string()
            }),
        };
        *self.challenge.lock().expect("Host challenge lock poisoned") = Some(challenge);
        Ok(false)
    }
}

pub struct Chain {
    sessions: Vec<client::Handle<Handler>>,
}

impl Chain {
    pub fn last(&self) -> &client::Handle<Handler> {
        self.sessions.last().expect("Validated nonempty chain")
    }
    pub fn is_closed(&self) -> bool {
        self.sessions.iter().any(|session| session.is_closed())
    }

    pub async fn close(&self) {
        for session in self.sessions.iter().rev() {
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                session.disconnect(russh::Disconnect::ByApplication, "", "en"),
            )
            .await;
        }
    }
}

pub async fn connect_chain(
    hops: &[Hop],
    cancel: &CancellationToken,
) -> Result<Chain, ConnectError> {
    connect_chain_inner(hops, cancel, None).await
}

pub async fn connect_remote_chain(
    hops: &[Hop],
    cancel: &CancellationToken,
) -> Result<(Chain, mpsc::Receiver<IncomingForward>), ConnectError> {
    let (sender, receiver) = mpsc::channel(128);
    let chain = connect_chain_inner(hops, cancel, Some(sender)).await?;
    Ok((chain, receiver))
}

async fn connect_chain_inner(
    hops: &[Hop],
    cancel: &CancellationToken,
    forwarded: Option<mpsc::Sender<IncomingForward>>,
) -> Result<Chain, ConnectError> {
    if hops.is_empty() {
        return Err(ConnectError::Invalid("Empty jumper chain".into()));
    }
    let mut chain = Chain { sessions: vec![] };
    for (index, hop) in hops.iter().enumerate() {
        let timeout = Duration::from_millis(if hop.jumper.timeout_ms == 0 {
            5000
        } else {
            hop.jumper.timeout_ms
        });
        let incoming = if index + 1 == hops.len() {
            forwarded.clone()
        } else {
            None
        };
        let operation = connect_hop(hop, chain.sessions.last(), incoming);
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ConnectError::Cancelled),
            result = tokio::time::timeout(timeout, operation) => result.unwrap_or(Err(ConnectError::Timeout)),
        };
        match result {
            Ok(session) => chain.sessions.push(session),
            Err(error) => {
                chain.close().await;
                return Err(error);
            }
        }
    }
    Ok(chain)
}

async fn connect_hop(
    hop: &Hop,
    previous: Option<&client::Handle<Handler>>,
    forwarded: Option<mpsc::Sender<IncomingForward>>,
) -> Result<client::Handle<Handler>, ConnectError> {
    let jumper = &hop.jumper;
    if jumper.host.is_empty() || jumper.port == 0 || jumper.user.is_empty() {
        return Err(ConnectError::Invalid(
            "Host, port and user are required".into(),
        ));
    }
    let challenge = Arc::new(Mutex::new(None));
    let denied = Arc::new(Mutex::new(None));
    let mut known_keys = hop.known_keys.clone();
    if let Some(key) = &hop.trusted_key {
        known_keys.trusted.push(key.clone());
    }
    let handler = Handler {
        jumper_id: jumper.id,
        host: jumper.host.clone(),
        port: jumper.port,
        known_keys,
        bypass: jumper.bypass_host_verification,
        challenge: challenge.clone(),
        forwarded,
        denied: denied.clone(),
    };
    let mut config = client::Config {
        nodelay: true,
        keepalive_interval: (jumper.keep_alive_interval_ms > 0)
            .then(|| Duration::from_millis(jumper.keep_alive_interval_ms)),
        keepalive_max: 3,
        ..Default::default()
    };
    if !jumper.host_key_algorithms.trim().is_empty() {
        let algorithms = jumper
            .host_key_algorithms
            .split(',')
            .map(|name| {
                name.trim()
                    .parse::<keys::Algorithm>()
                    .map_err(|_| ConnectError::Invalid("Invalid host-key algorithm".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        config.preferred.key = std::borrow::Cow::Owned(algorithms);
    }
    let config = Arc::new(config);
    let result = if let Some(previous) = previous {
        let stream = previous
            .channel_open_direct_tcpip(&jumper.host, jumper.port.into(), "127.0.0.1", 0)
            .await?
            .into_stream();
        client::connect_stream(config, stream, handler).await
    } else {
        client::connect(config, (jumper.host.as_str(), jumper.port), handler).await
    };
    if let Some(reason) = denied
        .lock()
        .expect("Host verification lock poisoned")
        .take()
    {
        return Err(ConnectError::Verification(reason));
    }
    if let Some(challenge) = challenge
        .lock()
        .expect("Host challenge lock poisoned")
        .take()
    {
        return Err(ConnectError::HostKey(Box::new(challenge)));
    }
    let mut session = result?;
    let authenticated = authenticate(&mut session, jumper).await;
    match authenticated {
        Ok(true) => Ok(session),
        result => {
            let _ = session
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
            result?;
            Err(ConnectError::Authentication)
        }
    }
}

async fn authenticate(
    session: &mut client::Handle<Handler>,
    jumper: &Jumper,
) -> Result<bool, ConnectError> {
    Ok(match jumper.auth_type.as_str() {
        "password" => session
            .authenticate_password(&jumper.user, &jumper.password)
            .await?
            .success(),
        "ssh_key" => {
            let path = expand_key_path(&jumper.key_path)?;
            let password = jumper.password.clone();
            // Encrypted-key KDF and disk I/O must not block cancellation or other tunnels.
            let key = tokio::task::spawn_blocking(move || {
                keys::load_secret_key(path, (!password.is_empty()).then_some(password.as_str()))
            })
            .await
            .map_err(|_| ConnectError::Invalid("Private key loader failed".into()))??;
            let hash = session.best_supported_rsa_hash().await?.flatten();
            session
                .authenticate_publickey(
                    &jumper.user,
                    keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await?
                .success()
        }
        "ssh_agent" => {
            #[cfg(windows)]
            let mut agent = keys::agent::client::AgentClient::connect_named_pipe(
                if jumper.agent_socket_path.is_empty() {
                    r"\\.\pipe\openssh-ssh-agent"
                } else {
                    &jumper.agent_socket_path
                },
            )
            .await?;
            #[cfg(unix)]
            let mut agent = if jumper.agent_socket_path.is_empty() {
                keys::agent::client::AgentClient::connect_env().await?
            } else {
                keys::agent::client::AgentClient::connect_uds(&jumper.agent_socket_path).await?
            };
            let hash = session.best_supported_rsa_hash().await?.flatten();
            for identity in agent.request_identities().await? {
                if let keys::agent::AgentIdentity::PublicKey { key, .. } = identity
                    && session
                        .authenticate_publickey_with(&jumper.user, key, hash, &mut agent)
                        .await
                        .map_err(|error| ConnectError::Agent(error.to_string()))?
                        .success()
                {
                    return Ok(true);
                }
            }
            false
        }
        _ => {
            return Err(ConnectError::Invalid(
                "Unknown authentication method".into(),
            ));
        }
    })
}

pub(crate) fn expand_key_path(path: &str) -> Result<std::path::PathBuf, ConnectError> {
    if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| ConnectError::Invalid("Cannot locate home directory".into()))?;
        return Ok(std::path::PathBuf::from(home).join(relative));
    }
    Ok(path.into())
}
