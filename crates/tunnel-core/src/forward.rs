use crate::{
    model::Tunnel,
    socks,
    ssh::{Chain, IncomingForward},
    traffic::{Counters, Metered},
};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub struct RemoteListener {
    address: String,
    port: u16,
    incoming: mpsc::Receiver<IncomingForward>,
}

impl RemoteListener {
    pub async fn bind(
        chain: &Chain,
        tunnel: &Tunnel,
        incoming: mpsc::Receiver<IncomingForward>,
    ) -> io::Result<Self> {
        if tunnel.mode != "remote" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected remote mode",
            ));
        }
        let assigned = tokio::time::timeout(
            Duration::from_secs(10),
            chain
                .last()
                .tcpip_forward(&tunnel.remote_host, tunnel.remote_port.into()),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Remote bind timed out"))?
        .map_err(io::Error::other)?;
        let port = if tunnel.remote_port == 0 {
            u16::try_from(assigned)
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| io::Error::other("Server returned invalid remote port"))?
        } else {
            tunnel.remote_port
        };
        Ok(Self {
            address: tunnel.remote_host.clone(),
            port,
            incoming,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn serve(
        mut self,
        chain: Arc<Chain>,
        tunnel: Tunnel,
        counters: Arc<Counters>,
        cancel: CancellationToken,
        errors: mpsc::Sender<String>,
    ) -> io::Result<()> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                Some(_) = connections.join_next(), if !connections.is_empty() => {},
                incoming = self.incoming.recv(), if connections.len() < 512 => {
                    let Some(incoming) = incoming else { break };
                    if incoming.port != u32::from(self.port) || !matching_bind_address(&self.address, &incoming.address) {
                        incoming.reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                        continue;
                    }
                    let tunnel = tunnel.clone(); let counters = counters.clone(); let errors = errors.clone();
                    connections.spawn(async move {
                        if let Err(error) = forward_remote_connection(incoming, &tunnel, counters).await {
                            let _ = errors.try_send(error.to_string());
                        }
                    });
                }
            }
        }
        self.incoming.close();
        connections.shutdown().await;
        // Even during cancellation, revoke the remote listener before permitting a restart.
        tokio::time::timeout(
            Duration::from_secs(3),
            chain
                .last()
                .cancel_tcpip_forward(self.address, self.port.into()),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Remote unbind timed out"))?
        .map_err(io::Error::other)
    }
}

fn matching_bind_address(bound: &str, connected: &str) -> bool {
    bound == connected
        || matches!(bound, "" | "*" | "0.0.0.0" | "::")
        || (bound == "localhost" && matches!(connected, "127.0.0.1" | "::1"))
}

async fn forward_remote_connection(
    incoming: IncomingForward,
    tunnel: &Tunnel,
    counters: Arc<Counters>,
) -> io::Result<()> {
    let socket = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect((tunnel.local_host.as_str(), tunnel.local_port)),
    )
    .await;
    let socket = match socket {
        Ok(Ok(socket)) => socket,
        result => {
            incoming
                .reply
                .reject(russh::ChannelOpenFailure::ConnectFailed)
                .await;
            return Err(match result {
                Ok(Err(error)) => error,
                Err(_) => io::Error::new(io::ErrorKind::TimedOut, "Local destination timed out"),
                Ok(Ok(_)) => unreachable!(),
            });
        }
    };
    incoming.reply.accept().await;
    let mut local = Metered::new(socket, counters);
    let mut remote = incoming.channel.into_stream();
    tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
    Ok(())
}

/// Serve L/S connections until cancelled. The caller owns reconnection and status events.
/// Every accepted stream is owned by this task set and is stopped before return.
pub async fn serve_local(
    listener: TcpListener,
    tunnel: Tunnel,
    chain: Arc<Chain>,
    counters: Arc<Counters>,
    cancel: CancellationToken,
    errors: mpsc::Sender<String>,
) -> io::Result<()> {
    if !["local", "dynamic"].contains(&tunnel.mode.as_str()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected local or dynamic mode",
        ));
    }
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Ok(()),
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept(), if connections.len() < 512 => {
                let (socket, origin) = match accepted { Ok(accepted) => accepted, Err(error) => break Err(error) };
                let chain = chain.clone(); let counters = counters.clone();
                let tunnel = tunnel.clone(); let errors = errors.clone();
                connections.spawn(async move {
                    if let Err(error) = forward_connection(socket, origin, &tunnel, &chain, counters).await {
                        // Bounded diagnostics must not stall traffic or grow without limit.
                        let _ = errors.try_send(error.to_string());
                    }
                });
            }
        }
    };
    connections.shutdown().await;
    result
}

async fn forward_connection(
    mut socket: TcpStream,
    origin: std::net::SocketAddr,
    tunnel: &Tunnel,
    chain: &Chain,
    counters: Arc<Counters>,
) -> io::Result<()> {
    let dynamic = tunnel.mode == "dynamic";
    let destination = if dynamic {
        tokio::time::timeout(Duration::from_secs(10), socks::negotiate(&mut socket))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SOCKS handshake timed out"))??
    } else {
        socks::Destination {
            host: tunnel.remote_host.clone(),
            port: tunnel.remote_port,
        }
    };
    let channel = tokio::time::timeout(
        Duration::from_secs(10),
        chain.last().channel_open_direct_tcpip(
            destination.host,
            destination.port.into(),
            origin.ip().to_string(),
            origin.port().into(),
        ),
    )
    .await;
    let mut channel = match channel {
        Ok(Ok(channel)) => channel.into_stream(),
        result => {
            if dynamic {
                socks::reply(&mut socket, 5).await?;
            }
            return Err(match result {
                Ok(Err(error)) => io::Error::other(error),
                Err(_) => io::Error::new(io::ErrorKind::TimedOut, "SSH destination timed out"),
                Ok(Ok(_)) => unreachable!(),
            });
        }
    };
    if dynamic {
        socks::reply(&mut socket, 0).await?;
    }
    let mut socket = Metered::new(socket, counters);
    tokio::io::copy_bidirectional(&mut socket, &mut channel).await?;
    Ok(())
}
