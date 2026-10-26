use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Jumper {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth_type: String,
    pub key_path: String,
    pub agent_socket_path: String,
    pub password: String,
    pub bypass_host_verification: bool,
    pub keep_alive_interval_ms: u64,
    pub timeout_ms: u64,
    pub host_key_algorithms: String,
    pub notes: String,
}

// Credentials deliberately have no Debug implementation.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Group {
    pub id: u64,
    pub name: String,
    pub tunnel_ids: Vec<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Tunnel {
    pub id: u64,
    pub name: String,
    pub group_id: u64,
    pub mode: String,
    pub jumper_ids: Vec<u64>,
    pub local_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub auto_start: bool,
    pub status: String,
    pub last_error: String,
    pub description: String,
    pub reconnect_mode: String,
    pub reconnect_interval_seconds: u64,
    pub reconnect_max_attempts: u64,
}

impl Default for Tunnel {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            group_id: 0,
            mode: "local".into(),
            jumper_ids: vec![],
            local_host: "127.0.0.1".into(),
            local_port: 0,
            remote_host: "127.0.0.1".into(),
            remote_port: 0,
            auto_start: false,
            status: "stopped".into(),
            last_error: String::new(),
            description: String::new(),
            // Missing policy denotes the legacy deadline, not unlimited retries.
            reconnect_mode: "legacy".into(),
            reconnect_interval_seconds: 5,
            reconnect_max_attempts: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub jumpers: Vec<Jumper>,
    pub groups: Vec<Group>,
    pub tunnels: Vec<Tunnel>,
    pub auto_run: bool,
    pub traffic_monitor_enabled: bool,
    pub trusted_hosts: Vec<crate::hostkeys::TrustedHost>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            jumpers: vec![],
            groups: vec![],
            tunnels: vec![],
            auto_run: false,
            traffic_monitor_enabled: true,
            trusted_hosts: vec![],
        }
    }
}

impl Config {
    pub fn normalize_loaded(&mut self) {
        for group in &mut self.groups {
            for tunnel in &self.tunnels {
                if tunnel.group_id == group.id && !group.tunnel_ids.contains(&tunnel.id) {
                    group.tunnel_ids.push(tunnel.id);
                }
            }
            group.tunnel_ids.sort_unstable();
            group.tunnel_ids.dedup();
        }
        for tunnel in &mut self.tunnels {
            // Membership has been imported above. Do not resurrect removed members on reload.
            tunnel.group_id = 0;
            tunnel.status = "stopped".into();
            tunnel.last_error.clear();
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for pin in &self.trusted_hosts {
            if pin.host.trim().is_empty()
                || pin.port == 0
                || russh::keys::PublicKey::from_openssh(&pin.public_key).is_err()
            {
                return Err("Invalid trusted host record".into());
            }
        }
        if self.version != 1 {
            return Err("Unsupported configuration version".into());
        }
        fn ids(values: impl Iterator<Item = u64>) -> Result<HashSet<u64>, String> {
            let mut set = HashSet::new();
            for id in values {
                if id == 0 || id > 9_007_199_254_740_991 || !set.insert(id) {
                    return Err("Invalid or duplicate ID".into());
                }
            }
            Ok(set)
        }
        let jumpers = ids(self.jumpers.iter().map(|j| j.id))?;
        let tunnels = ids(self.tunnels.iter().map(|t| t.id))?;
        ids(self.groups.iter().map(|g| g.id))?;
        for jumper in &self.jumpers {
            if jumper.host.trim().is_empty() || jumper.user.trim().is_empty() || jumper.port == 0 {
                return Err("Jumper requires host, user and nonzero port".into());
            }
            if !["password", "ssh_key", "ssh_agent"].contains(&jumper.auth_type.as_str()) {
                return Err("Unsupported authentication method".into());
            }
        }
        for tunnel in &self.tunnels {
            if tunnel.name.trim().is_empty()
                || tunnel.local_host.trim().is_empty()
                || (tunnel.mode != "dynamic" && tunnel.remote_host.trim().is_empty())
            {
                return Err("Tunnel requires a name and endpoint hosts".into());
            }
            if !["local", "remote", "dynamic"].contains(&tunnel.mode.as_str()) {
                return Err("Unsupported forwarding mode".into());
            }
            if tunnel.jumper_ids.is_empty()
                || tunnel.jumper_ids.iter().any(|id| !jumpers.contains(id))
            {
                return Err("Tunnel references missing jumpers".into());
            }
            if tunnel
                .jumper_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                != tunnel.jumper_ids.len()
            {
                return Err("Jumper chain contains duplicate hops".into());
            }
            if tunnel.local_port == 0 || (tunnel.mode != "dynamic" && tunnel.remote_port == 0) {
                return Err("Port must be nonzero".into());
            }
            crate::reconnect::Policy::from_tunnel(tunnel)?;
        }
        for group in &self.groups {
            if group.name.trim().is_empty()
                || group.tunnel_ids.iter().any(|id| !tunnels.contains(id))
            {
                return Err("Invalid group membership or name".into());
            }
        }
        Ok(())
    }
}
