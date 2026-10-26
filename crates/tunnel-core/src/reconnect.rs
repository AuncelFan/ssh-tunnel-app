use crate::model::Tunnel;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub enum Policy {
    Disabled,
    Legacy,
    Fixed { interval: Duration, attempts: u64 },
    Exponential { interval: Duration, attempts: u64 },
}

impl Policy {
    pub fn from_tunnel(t: &Tunnel) -> Result<Self, String> {
        let interval = Duration::from_secs(t.reconnect_interval_seconds);
        let attempts = t.reconnect_max_attempts;
        match t.reconnect_mode.as_str() {
            "" | "legacy" => Ok(Self::Legacy),
            "disabled" => Ok(Self::Disabled),
            "fixed" | "exponential"
                if (1..=86400).contains(&interval.as_secs()) && attempts <= 1_000_000 =>
            {
                Ok(if t.reconnect_mode == "fixed" {
                    Self::Fixed { interval, attempts }
                } else {
                    Self::Exponential { interval, attempts }
                })
            }
            _ => Err("Invalid reconnect policy".into()),
        }
    }

    // Attempt numbers start at one. None means the outage's retry budget expired.
    pub fn delay(&self, attempt: u64, elapsed: Duration) -> Option<Duration> {
        if attempt == 0 {
            return None;
        }
        match self {
            Self::Disabled => None,
            Self::Legacy => {
                let remaining = Duration::from_secs(900).checked_sub(elapsed)?;
                if remaining.is_zero() {
                    return None;
                }
                Some(
                    Duration::from_millis(500)
                        .saturating_mul(1u32 << (attempt - 1).min(7))
                        .min(Duration::from_secs(60))
                        .min(remaining),
                )
            }
            Self::Fixed { interval, attempts } | Self::Exponential { interval, attempts } => {
                if *attempts != 0 && attempt > *attempts {
                    return None;
                }
                if matches!(self, Self::Fixed { .. }) {
                    return Some(*interval);
                }
                Some(
                    interval
                        .saturating_mul(1u32 << (attempt - 1).min(31))
                        .min((*interval).max(Duration::from_secs(60))),
                )
            }
        }
    }
}

pub async fn wait(delay: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! { biased; _ = cancel.cancelled() => false, _ = tokio::time::sleep(delay) => true }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_and_exponential_have_no_hidden_deadline() {
        let elapsed = Duration::from_secs(1_000_000);
        let p = Policy::Exponential {
            interval: Duration::from_secs(5),
            attempts: 0,
        };
        assert_eq!(
            (1..=6)
                .map(|i| p.delay(i, elapsed).unwrap().as_secs())
                .collect::<Vec<_>>(),
            vec![5, 10, 20, 40, 60, 60]
        );
        assert_eq!(
            Policy::Fixed {
                interval: Duration::from_secs(120),
                attempts: 0
            }
            .delay(1_000_001, elapsed),
            Some(Duration::from_secs(120))
        );
        assert!(Policy::Legacy.delay(1, elapsed).is_none());
    }
    #[test]
    fn finite_limit_and_long_interval() {
        let p = Policy::Exponential {
            interval: Duration::from_secs(120),
            attempts: 2,
        };
        assert_eq!(p.delay(2, Duration::ZERO), Some(Duration::from_secs(120)));
        assert!(p.delay(3, Duration::ZERO).is_none());
    }
    #[tokio::test]
    async fn cancelled_wait_does_not_sleep() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            !tokio::time::timeout(
                Duration::from_millis(100),
                wait(Duration::from_secs(3600), &cancel)
            )
            .await
            .unwrap()
        );
    }
}
