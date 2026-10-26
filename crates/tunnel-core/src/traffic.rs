use serde::Serialize;
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Instant,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Default)]
pub struct Counters {
    uploaded: AtomicU64,
    downloaded: AtomicU64,
    recent: Mutex<RollingHour>,
}

struct RollingHour {
    started: Instant,
    buckets: VecDeque<(u64, Totals)>,
    total: Totals,
}

impl Default for RollingHour {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            buckets: VecDeque::new(),
            total: Totals::default(),
        }
    }
}

impl RollingHour {
    // One-second buckets bound memory independently of transfer rate or UI polling.
    fn expire(&mut self, second: u64) {
        while self
            .buckets
            .front()
            .is_some_and(|(time, _)| second.saturating_sub(*time) >= 3600)
        {
            let (_, expired) = self.buckets.pop_front().unwrap();
            self.total.uploaded -= expired.uploaded;
            self.total.downloaded -= expired.downloaded;
        }
    }

    fn record(&mut self, second: u64, delta: Totals) {
        self.expire(second);
        if self.buckets.back().is_none_or(|(time, _)| *time != second) {
            self.buckets.push_back((second, Totals::default()));
        }
        let (_, bucket) = self.buckets.back_mut().unwrap();
        bucket.uploaded += delta.uploaded;
        bucket.downloaded += delta.downloaded;
        self.total.uploaded += delta.uploaded;
        self.total.downloaded += delta.downloaded;
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub uploaded: u64,
    pub downloaded: u64,
}

impl Counters {
    fn record(&self, uploaded: u64, downloaded: u64) {
        if uploaded == 0 && downloaded == 0 {
            return;
        }
        self.uploaded.fetch_add(uploaded, Ordering::Relaxed);
        self.downloaded.fetch_add(downloaded, Ordering::Relaxed);
        let mut recent = self.recent.lock().expect("Traffic history lock poisoned");
        let second = recent.started.elapsed().as_secs();
        recent.record(
            second,
            Totals {
                uploaded,
                downloaded,
            },
        );
    }

    pub fn last_hour(&self) -> Totals {
        let mut recent = self.recent.lock().expect("Traffic history lock poisoned");
        let second = recent.started.elapsed().as_secs();
        recent.expire(second);
        recent.total
    }

    pub fn snapshot(&self) -> Totals {
        Totals {
            uploaded: self.uploaded.load(Ordering::Relaxed),
            downloaded: self.downloaded.load(Ordering::Relaxed),
        }
    }
}

/// Wrap the local socket, so traffic direction is consistent across L/R/S modes.
/// Counting occurs during I/O, not only after a connection closes.
pub struct Metered<S> {
    stream: S,
    counters: Arc<Counters>,
}

impl<S> Metered<S> {
    pub fn new(stream: S, counters: Arc<Counters>) -> Self {
        Self { stream, counters }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Metered<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.stream).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = result {
            this.counters
                .record((buf.filled().len() - before) as u64, 0);
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Metered<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.stream).poll_write(cx, buf);
        if let Poll::Ready(Ok(count)) = result {
            this.counters.record(0, count as u64);
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn rolling_hour_expires_idle_data_and_keeps_at_most_3600_buckets() {
        let mut hour = RollingHour::default();
        hour.record(
            0,
            Totals {
                uploaded: 10,
                downloaded: 2,
            },
        );
        hour.record(
            0,
            Totals {
                uploaded: 5,
                downloaded: 3,
            },
        );
        hour.record(
            3599,
            Totals {
                uploaded: 7,
                downloaded: 9,
            },
        );
        assert_eq!(
            hour.total,
            Totals {
                uploaded: 22,
                downloaded: 14
            }
        );
        hour.expire(3600);
        assert_eq!(
            hour.total,
            Totals {
                uploaded: 7,
                downloaded: 9
            }
        );
        hour.expire(7199);
        assert_eq!(hour.total, Totals::default());
        assert!(hour.buckets.is_empty());
        for second in 7200..17200 {
            hour.record(
                second,
                Totals {
                    uploaded: 1,
                    downloaded: 2,
                },
            );
        }
        assert_eq!(hour.buckets.len(), 3600);
        assert_eq!(
            hour.total,
            Totals {
                uploaded: 3600,
                downloaded: 7200
            }
        );
    }

    #[test]
    fn rolling_expiry_does_not_reset_lifetime_rate_counters() {
        let counters = Counters::default();
        counters.record(100, 200);
        assert_eq!(counters.last_hour(), counters.snapshot());
        counters.recent.lock().unwrap().started =
            Instant::now() - std::time::Duration::from_secs(3601);
        assert_eq!(counters.last_hour(), Totals::default());
        assert_eq!(
            counters.snapshot(),
            Totals {
                uploaded: 100,
                downloaded: 200
            }
        );
        counters.record(3, 4);
        assert_eq!(
            counters.last_hour(),
            Totals {
                uploaded: 3,
                downloaded: 4
            }
        );
        assert_eq!(
            counters.snapshot(),
            Totals {
                uploaded: 103,
                downloaded: 204
            }
        );
    }

    #[tokio::test]
    async fn live_counters_are_scoped_to_a_tunnel_and_preserve_half_close() {
        let totals = Arc::new(Counters::default());
        let unrelated = Counters::default();
        let (mut peer, socket) = tokio::io::duplex(64);
        let mut metered = Metered::new(socket, totals.clone());
        peer.write_all(b"upload").await.unwrap();
        let mut bytes = [0; 6];
        metered.read_exact(&mut bytes).await.unwrap();
        assert_eq!(
            totals.snapshot(),
            Totals {
                uploaded: 6,
                downloaded: 0
            }
        );
        peer.shutdown().await.unwrap();
        assert_eq!(metered.read(&mut bytes).await.unwrap(), 0);
        metered.write_all(b"download").await.unwrap();
        metered.shutdown().await.unwrap();
        let mut response = vec![];
        peer.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"download");
        assert_eq!(
            totals.snapshot(),
            Totals {
                uploaded: 6,
                downloaded: 8
            }
        );
        assert_eq!(unrelated.snapshot(), Totals::default());
        assert_eq!(totals.last_hour(), totals.snapshot());
        assert_eq!(unrelated.last_hour(), Totals::default());
    }
}
