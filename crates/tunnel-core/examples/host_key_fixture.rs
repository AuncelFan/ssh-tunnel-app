//! Loopback-only SSH fixture for native UI verification; never packaged with the application.
use russh::{
    keys::{Algorithm, HashAlg, PrivateKey},
    server,
};
use std::{io::BufRead, sync::Arc};
use tokio::net::TcpListener;

struct Fixture;
impl server::Handler for Fixture {
    type Error = russh::Error;
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if host != "127.0.0.1" || port > 65535 {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        match tokio::net::TcpStream::connect((host, port as u16)).await {
            Ok(mut socket) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut channel.into_stream())
                        .await;
                });
            }
            Err(_) => {
                reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
            }
        }
        Ok(())
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if user == "fixture" && password == "fixture-only" {
            server::Auth::Accept
        } else {
            server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            }
        })
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    println!(
        "{}",
        serde_json::json!({"port": port, "fingerprint": key.public_key().fingerprint(HashAlg::Sha256).to_string()})
    );
    let (sender, mut commands) = tokio::sync::mpsc::channel(4);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.blocking_send(line).is_err() {
                break;
            }
        }
    });
    let mut sessions = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = sessions.join_next(), if !sessions.is_empty() => {},
            command = commands.recv() => {
                match command.as_deref() {
                    Some("rotate") => {
                        key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
                        println!("{}", serde_json::json!({"rotated": true, "fingerprint": key.public_key().fingerprint(HashAlg::Sha256).to_string()}));
                    }
                    _ => break,
                }
            }
            accepted = listener.accept() => {
                let (socket, _) = accepted.unwrap();
                let config = Arc::new(server::Config { keys: vec![key.clone()], ..Default::default() });
                sessions.spawn(async move { if let Ok(session) = server::run_stream(config, socket, Fixture).await { let _ = session.await; } });
            }
        }
    }
}
