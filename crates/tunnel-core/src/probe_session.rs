use crate::{
    model::{Jumper, Tunnel},
    ssh::HostKeyChallenge,
};
use serde::Deserialize;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Deserialize)]
pub struct Request {
    pub jumpers: Vec<Jumper>,
    pub tunnel: Option<Tunnel>,
    pub diagnostics: bool,
}

struct Session {
    token: u64,
    request: Request,
    cancel: CancellationToken,
    running: bool,
    challenge: Option<HostKeyChallenge>,
}

#[derive(Default)]
pub struct Sessions {
    finished: tokio::sync::Notify,
    generation: AtomicU64,
    current: Mutex<Option<Session>>,
}

impl Sessions {
    pub fn begin(&self, request: Request) -> Result<u64, String> {
        let mut current = self.current.lock().expect("Probe lock poisoned");
        if current.as_ref().is_some_and(|session| session.running) {
            return Err(
                "A connection test is still running; cancel it and wait for cleanup".into(),
            );
        }
        let token = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        *current = Some(Session {
            token,
            request,
            cancel: CancellationToken::new(),
            running: false,
            challenge: None,
        });
        Ok(token)
    }

    pub fn claim(&self, token: u64) -> Result<(Request, CancellationToken), String> {
        let mut current = self.current.lock().expect("Probe lock poisoned");
        let session = current
            .as_mut()
            .filter(|session| {
                session.token == token && !session.cancel.is_cancelled() && !session.running
            })
            .ok_or("Connection test is no longer available")?;
        session.running = true;
        session.challenge = None;
        Ok((session.request.clone(), session.cancel.clone()))
    }

    pub fn finish(&self, token: u64, challenge: Option<HostKeyChallenge>) {
        let mut current = self.current.lock().expect("Probe lock poisoned");
        if let Some(session) = current.as_mut().filter(|session| session.token == token) {
            session.running = false;
            if session.cancel.is_cancelled() {
                *current = None;
            } else {
                session.challenge = challenge;
            }
        }
        self.finished.notify_waiters();
    }

    pub async fn wait_idle(&self) {
        loop {
            let notified = self.finished.notified();
            if !self
                .current
                .lock()
                .expect("Probe lock poisoned")
                .as_ref()
                .is_some_and(|session| session.running)
            {
                return;
            }
            notified.await;
        }
    }

    pub fn challenge(&self, token: u64) -> Result<HostKeyChallenge, String> {
        let current = self.current.lock().expect("Probe lock poisoned");
        current
            .as_ref()
            .filter(|session| {
                session.token == token && !session.running && !session.cancel.is_cancelled()
            })
            .and_then(|session| session.challenge.clone())
            .ok_or_else(|| "Test host-key request is no longer active".into())
    }

    pub fn cancel(&self, token: Option<u64>) {
        let mut current = self.current.lock().expect("Probe lock poisoned");
        if let Some(session) = current
            .as_mut()
            .filter(|session| token.is_none_or(|token| session.token == token))
        {
            session.cancel.cancel();
            session.challenge = None;
            if !session.running {
                *current = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            jumpers: vec![],
            tunnel: None,
            diagnostics: false,
        }
    }
    fn challenge() -> HostKeyChallenge {
        HostKeyChallenge {
            jumper_id: 0,
            host: "example.invalid".into(),
            port: 22,
            previous_fingerprint: None,
            fingerprint: "fixture".into(),
            public_key: "fixture".into(),
            authority_fingerprint: None,
        }
    }

    #[tokio::test]
    async fn cancelled_and_superseded_tokens_cannot_republish_or_approve_challenges() {
        let sessions = Sessions::default();
        let first = sessions.begin(request()).unwrap();
        let (_, cancel) = sessions.claim(first).unwrap();
        assert!(sessions.begin(request()).is_err());
        assert!(sessions.claim(first).is_err());
        sessions.cancel(Some(first));
        assert!(cancel.is_cancelled());
        sessions.finish(first, Some(challenge()));
        sessions.wait_idle().await;
        assert!(sessions.challenge(first).is_err());
        let second = sessions.begin(request()).unwrap();
        sessions.cancel(Some(first));
        sessions.claim(second).unwrap();
        sessions.finish(second, Some(challenge()));
        assert!(sessions.challenge(second).is_ok());
        assert!(sessions.challenge(first).is_err());
        sessions.cancel(None);
        assert!(sessions.challenge(second).is_err());
    }
}
