use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, PartialEq, Eq)]
pub struct Destination {
    pub host: String,
    pub port: u16,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Negotiate SOCKS5 CONNECT. Hostnames remain unresolved for remote SSH-side DNS.
/// The caller must impose a handshake timeout and restrict exposed listener addresses.
pub async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
) -> io::Result<Destination> {
    let version = stream.read_u8().await?;
    if version != 5 {
        return Err(invalid("Unsupported SOCKS version"));
    }
    let count = stream.read_u8().await? as usize;
    let mut methods = vec![0; count];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        stream.write_all(&[5, 255]).await?;
        return Err(invalid("SOCKS client does not allow no-authentication"));
    }
    stream.write_all(&[5, 0]).await?;
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 || header[2] != 0 {
        return Err(invalid("Invalid SOCKS request"));
    }
    if header[1] != 1 {
        reply(stream, 7).await?;
        return Err(invalid("Only SOCKS CONNECT is supported"));
    }
    let host = match header[3] {
        1 => {
            let mut bytes = [0; 4];
            stream.read_exact(&mut bytes).await?;
            Ipv4Addr::from(bytes).to_string()
        }
        4 => {
            let mut bytes = [0; 16];
            stream.read_exact(&mut bytes).await?;
            Ipv6Addr::from(bytes).to_string()
        }
        3 => {
            let len = stream.read_u8().await? as usize;
            let mut bytes = vec![0; len];
            stream.read_exact(&mut bytes).await?;
            if bytes.is_empty() || bytes.iter().any(|b| *b <= 32 || *b >= 127) {
                reply(stream, 8).await?;
                return Err(invalid("Invalid SOCKS domain name"));
            }
            String::from_utf8(bytes).map_err(|_| invalid("Invalid SOCKS domain name"))?
        }
        _ => {
            reply(stream, 8).await?;
            return Err(invalid("Unsupported SOCKS address type"));
        }
    };
    let port = stream.read_u16().await?;
    if port == 0 {
        reply(stream, 1).await?;
        return Err(invalid("Invalid destination port"));
    }
    Ok(Destination { host, port })
}

/// Success must only be sent after the SSH destination channel opens successfully.
pub async fn reply<S: AsyncWrite + Unpin>(stream: &mut S, status: u8) -> io::Result<()> {
    stream
        .write_all(&[5, status, 0, 1, 0, 0, 0, 0, 0, 0])
        .await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn domain_is_not_resolved_locally() {
        let (mut client, mut server) = tokio::io::duplex(256);
        client
            .write_all(&[
                5, 1, 0, 5, 1, 0, 3, 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 1, 187,
            ])
            .await
            .unwrap();
        assert_eq!(
            negotiate(&mut server).await.unwrap(),
            Destination {
                host: "example".into(),
                port: 443
            }
        );
        let mut method = [0; 2];
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [5, 0]);
        // No success reply is emitted before a destination is actually connected.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), client.read_u8())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unsupported_udp_request_is_rejected() {
        let (mut client, mut server) = tokio::io::duplex(64);
        client.write_all(&[5, 1, 0, 5, 3, 0, 1]).await.unwrap();
        assert!(negotiate(&mut server).await.is_err());
        let mut response = [0; 12];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(&response[..4], &[5, 0, 5, 7]);
    }

    #[tokio::test]
    async fn ipv6_and_ipv4_are_supported() {
        for (kind, bytes, expected) in [
            (1, vec![127, 0, 0, 1], "127.0.0.1"),
            (
                4,
                vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                "::1",
            ),
        ] {
            let (mut client, mut server) = tokio::io::duplex(64);
            client.write_all(&[5, 1, 0, 5, 1, 0, kind]).await.unwrap();
            client.write_all(&bytes).await.unwrap();
            client.write_all(&[0, 80]).await.unwrap();
            assert_eq!(negotiate(&mut server).await.unwrap().host, expected);
        }
    }
}
