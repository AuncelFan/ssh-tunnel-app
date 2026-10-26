use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{
    HashAlg, PublicKey, PublicKeyOrCertificate,
    ssh_key::{
        certificate::CertType,
        known_hosts::{HostPatterns, Marker},
    },
};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedHost {
    pub host: String,
    pub port: u16,
    pub public_key: String,
}

#[derive(Clone, Default)]
pub struct HostKeys {
    pub trusted: Vec<PublicKey>,
    pub revoked: Vec<PublicKey>,
    pub authorities: Vec<PublicKey>,
    source: Option<(std::path::PathBuf, Vec<TrustedHost>)>,
}

pub enum Verification {
    Trusted,
    Untrusted,
    Revoked,
    InvalidCertificate,
}

impl HostKeys {
    pub fn refresh(&self, host: &str, port: u16) -> Result<Self, String> {
        match &self.source {
            Some((path, pins)) => Self::load(path, host, port, pins),
            None => Ok(self.clone()),
        }
    }
    pub fn verify(&self, host: &str, offered: &PublicKeyOrCertificate) -> Verification {
        let key = offered.public_key();
        if self.revoked.iter().any(|revoked| {
            revoked.key_data() == key.key_data()
                || offered
                    .certificate()
                    .is_some_and(|cert| revoked.key_data() == cert.signature_key())
        }) {
            return Verification::Revoked;
        }
        if let Some(cert) = offered.certificate() {
            let signing_fingerprint = cert.signature_key().fingerprint(HashAlg::Sha256);
            if cert.cert_type() != CertType::Host
                || !cert.critical_options().is_empty()
                || (!cert.valid_principals().is_empty()
                    && !cert
                        .valid_principals()
                        .iter()
                        .any(|principal| principal.eq_ignore_ascii_case(host)))
                || cert.validate([&signing_fingerprint]).is_err()
            {
                return Verification::InvalidCertificate;
            }
            // Validate certificate structure/time/signature independently of trust. An explicit
            // server-key pin trusts only that key, not every host signed by an unknown CA.
            if self
                .authorities
                .iter()
                .any(|authority| authority.key_data() == cert.signature_key())
                || self
                    .trusted
                    .iter()
                    .any(|trusted| trusted.key_data() == key.key_data())
            {
                return Verification::Trusted;
            }
            return Verification::Untrusted;
        }
        if self
            .trusted
            .iter()
            .any(|trusted| trusted.key_data() == key.key_data())
        {
            Verification::Trusted
        } else {
            Verification::Untrusted
        }
    }

    pub fn load(path: &Path, host: &str, port: u16, pins: &[TrustedHost]) -> Result<Self, String> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err("Cannot read known_hosts".into()),
        };
        let mut keys = Self::parse(&content, host, port)?;
        for pin in pins
            .iter()
            .filter(|pin| pin.host.eq_ignore_ascii_case(host) && pin.port == port)
        {
            let key =
                PublicKey::from_openssh(&pin.public_key).map_err(|_| "Invalid saved host key")?;
            // An explicit app approval supersedes older keys of this algorithm, never revocations.
            keys.trusted
                .retain(|old| old.algorithm() != key.algorithm());
            keys.trusted.push(key);
        }
        keys.source = Some((path.to_path_buf(), pins.to_vec()));
        Ok(keys)
    }

    pub fn parse(content: &str, host: &str, port: u16) -> Result<Self, String> {
        let mut result = Self::default();
        let endpoint = if port == 22 {
            host.to_lowercase()
        } else {
            format!("[{}]:{port}", host.to_lowercase())
        };
        for (index, line) in content.lines().enumerate() {
            let words: Vec<_> = line.split_whitespace().collect();
            if words.is_empty() || words[0].starts_with('#') {
                continue;
            }
            let offset = usize::from(words[0].starts_with('@'));
            let Some(patterns) = words.get(offset) else {
                continue;
            };
            let patterns: HostPatterns = patterns
                .parse()
                .map_err(|_| format!("Invalid known_hosts pattern at line {}", index + 1))?;
            if !matches_host(&patterns, &endpoint) {
                continue;
            }
            if words.len() < offset + 3 {
                return Err(format!("Invalid known_hosts entry at line {}", index + 1));
            }
            let key =
                PublicKey::from_openssh(&format!("{} {}", words[offset + 1], words[offset + 2]))
                    .map_err(|_| format!("Invalid known_hosts key at line {}", index + 1))?;
            match if offset == 1 {
                Some(
                    words[0]
                        .parse::<Marker>()
                        .map_err(|_| "Unsupported known_hosts marker")?,
                )
            } else {
                None
            } {
                Some(Marker::Revoked) => result.revoked.push(key),
                Some(Marker::CertAuthority) => result.authorities.push(key),
                None => result.trusted.push(key),
            }
        }
        Ok(result)
    }
}

fn matches_host(patterns: &HostPatterns, endpoint: &str) -> bool {
    match patterns {
        HostPatterns::HashedName { salt, hash } => {
            Hmac::<Sha1>::new_from_slice(salt).is_ok_and(|mac| {
                mac.chain_update(endpoint.as_bytes())
                    .verify_slice(hash)
                    .is_ok()
            })
        }
        HostPatterns::Patterns(patterns) => {
            let mut matched = false;
            for pattern in patterns {
                if let Some(negated) = pattern.strip_prefix('!') {
                    if wildcard(&negated.to_lowercase(), endpoint) {
                        return false;
                    }
                } else if wildcard(&pattern.to_lowercase(), endpoint) {
                    matched = true;
                }
            }
            matched
        }
    }
}

pub(crate) fn wildcard(pattern: &str, value: &str) -> bool {
    let (pattern, value) = (pattern.as_bytes(), value.as_bytes());
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(position) = star {
            p = position + 1;
            retry += 1;
            v = retry;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey, ssh_key::certificate::Builder};

    fn key() -> PrivateKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
    }

    #[test]
    fn patterns_ports_whitespace_and_negations_match_openssh_semantics() {
        let key = key();
        let public = key.public_key().to_openssh().unwrap();
        let content = format!(
            "# comment\n*.example.org,!blocked.example.org\t{public}\n[port.example.org]:2222 {public}\n"
        );
        assert_eq!(
            HostKeys::parse(&content, "TEST.EXAMPLE.ORG", 22)
                .unwrap()
                .trusted
                .len(),
            1
        );
        assert!(
            HostKeys::parse(&content, "blocked.example.org", 22)
                .unwrap()
                .trusted
                .is_empty()
        );
        assert_eq!(
            HostKeys::parse(&content, "port.example.org", 2222)
                .unwrap()
                .trusted
                .len(),
            1
        );
        assert!(
            HostKeys::parse(&content, "port.example.org", 2223)
                .unwrap()
                .trusted
                .is_empty()
        );
    }

    #[test]
    fn hashed_endpoint_matches_only_its_exact_host_and_port() {
        let salt = vec![7; 20];
        let hash: [u8; 20] = Hmac::<Sha1>::new_from_slice(&salt)
            .unwrap()
            .chain_update(b"[example.org]:2222")
            .finalize()
            .into_bytes()
            .into();
        let pattern = HostPatterns::HashedName { salt, hash }.to_string();
        let content = format!("{pattern} {}", key().public_key().to_openssh().unwrap());
        assert_eq!(
            HostKeys::parse(&content, "example.org", 2222)
                .unwrap()
                .trusted
                .len(),
            1
        );
        assert!(
            HostKeys::parse(&content, "example.org", 22)
                .unwrap()
                .trusted
                .is_empty()
        );
        assert!(
            HostKeys::parse(&content, "other.org", 2222)
                .unwrap()
                .trusted
                .is_empty()
        );
    }

    #[test]
    fn replacement_overrides_old_key_but_never_revocation_and_does_not_write_system_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let old = key();
        let new = key();
        let content = format!(
            "example.org {}\n@revoked example.org {}\n",
            old.public_key().to_openssh().unwrap(),
            new.public_key().to_openssh().unwrap()
        );
        std::fs::write(&path, &content).unwrap();
        let pin = TrustedHost {
            host: "example.org".into(),
            port: 22,
            public_key: new.public_key().to_openssh().unwrap(),
        };
        let keys = HostKeys::load(&path, "example.org", 22, &[pin]).unwrap();
        assert!(matches!(
            keys.verify("example.org", &new.public_key().clone().into()),
            Verification::Revoked
        ));
        assert!(matches!(
            keys.verify("example.org", &old.public_key().clone().into()),
            Verification::Untrusted
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), content);
    }

    #[test]
    fn certificate_requires_trusted_ca_or_exact_pin_and_valid_principal_and_time() {
        let ca = key();
        let host = key();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut builder =
            Builder::new_with_random_nonce(&mut rand::rng(), host.public_key(), now - 1, now + 100)
                .unwrap();
        builder.cert_type(CertType::Host).unwrap();
        builder.valid_principal("example.org").unwrap();
        let cert = PublicKeyOrCertificate::Certificate(builder.sign(&ca).unwrap());
        let mut keys = HostKeys {
            authorities: vec![ca.public_key().clone()],
            ..Default::default()
        };
        assert!(matches!(
            keys.verify("example.org", &cert),
            Verification::Trusted
        ));
        assert!(matches!(
            keys.verify("different.org", &cert),
            Verification::InvalidCertificate
        ));
        keys.authorities.clear();
        assert!(matches!(
            keys.verify("example.org", &cert),
            Verification::Untrusted
        ));
        keys.trusted.push(host.public_key().clone());
        assert!(matches!(
            keys.verify("example.org", &cert),
            Verification::Trusted
        ));
        let mut expired =
            Builder::new_with_random_nonce(&mut rand::rng(), host.public_key(), now - 100, now - 1)
                .unwrap();
        expired.cert_type(CertType::Host).unwrap();
        expired.valid_principal("example.org").unwrap();
        assert!(matches!(
            keys.verify(
                "example.org",
                &PublicKeyOrCertificate::Certificate(expired.sign(&ca).unwrap())
            ),
            Verification::InvalidCertificate
        ));
        keys.authorities.push(ca.public_key().clone());
        keys.revoked.push(ca.public_key().clone());
        assert!(matches!(
            keys.verify("example.org", &cert),
            Verification::Revoked
        ));
    }

    #[test]
    fn malformed_matching_entries_and_unreadable_files_fail_closed() {
        assert!(HostKeys::parse("example.org ssh-ed25519 invalid", "example.org", 22).is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(HostKeys::load(dir.path(), "example.org", 22, &[]).is_err());
    }

    #[test]
    fn refresh_observes_revocations_added_after_initial_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let key = key();
        let public = key.public_key().to_openssh().unwrap();
        std::fs::write(&path, format!("example.org {public}")).unwrap();
        let initial = HostKeys::load(&path, "example.org", 22, &[]).unwrap();
        assert!(matches!(
            initial.verify("example.org", &key.public_key().clone().into()),
            Verification::Trusted
        ));
        std::fs::write(&path, format!("@revoked example.org {public}")).unwrap();
        assert!(matches!(
            initial
                .refresh("example.org", 22)
                .unwrap()
                .verify("example.org", &key.public_key().clone().into()),
            Verification::Revoked
        ));
    }
}
