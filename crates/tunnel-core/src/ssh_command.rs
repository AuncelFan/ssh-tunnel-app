use crate::model::{Jumper, Tunnel};
use serde::Serialize;

#[derive(Serialize)]
pub struct CommandPreview {
    pub jumpers: Vec<Jumper>,
    pub tunnels: Vec<Tunnel>,
    pub warnings: Vec<String>,
}

/// Parse only. Never spawn ssh, invoke a shell, expand command substitution or run remote commands.
pub fn parse(command: &str) -> Result<CommandPreview, String> {
    if command.len() > 65_536 {
        return Err("SSH command is too long".into());
    }
    let tokens = crate::ssh_config::words(command)?;
    let executable = tokens
        .first()
        .and_then(|word| word.rsplit(['/', '\\']).next())
        .unwrap_or("");
    if !executable.eq_ignore_ascii_case("ssh") && !executable.eq_ignore_ascii_case("ssh.exe") {
        return Err("Enter a single ssh command".into());
    }
    let mut forwards = Vec::new();
    let mut target = None;
    let mut port = None;
    let mut user = None;
    let mut identity = None;
    let mut proxy = None;
    let mut keepalive = 5000;
    let mut timeout = 10000;
    let mut bypass = false;
    let mut warnings = Vec::new();
    let mut seen_options = std::collections::HashSet::new();
    let mut index = 1;
    while index < tokens.len() {
        let token = &tokens[index];
        if !token.starts_with('-') {
            if target.is_some() {
                return Err("Remote commands and shell expressions are not imported".into());
            }
            if token.contains([';', '|', '&', '`', '$', '>', '<']) {
                return Err("Shell expressions are not imported".into());
            }
            target = Some(token.clone());
            index += 1;
            continue;
        }
        if token.len() < 2 {
            return Err("Invalid SSH option".into());
        }
        for (offset, option) in token[1..].char_indices() {
            if "NfTCv46qnxYXAa".contains(option) {
                if "C46xYXAa".contains(option) {
                    warnings.push("runtime_option_not_imported".into());
                }
                continue;
            }
            if !"LRDpliJo".contains(option) {
                return Err(format!("Unsupported SSH option: -{option}"));
            }
            let rest = &token[1 + offset + option.len_utf8()..];
            let value = if rest.is_empty() {
                index += 1;
                tokens
                    .get(index)
                    .ok_or("SSH option requires a value")?
                    .clone()
            } else {
                rest.into()
            };
            match option {
                'L' | 'R' | 'D' => forwards.push((option, value)),
                'p' => {
                    if port.is_none() {
                        port = Some(parse_port(&value)?);
                    }
                }
                'l' => {
                    if user.is_none() {
                        user = Some(value);
                    }
                }
                'i' => {
                    if identity.is_some() {
                        warnings.push("multiple_identity_files".into());
                    } else {
                        identity = Some(value);
                    }
                }
                'J' => {
                    if proxy.is_none() {
                        proxy = Some(value);
                    }
                }
                'o' => {
                    let (key, value) = value.split_once(['=', ' ']).ok_or("Use -o Key=Value")?;
                    let value = value.trim();
                    if !seen_options.insert(key.to_ascii_lowercase()) {
                        break;
                    }
                    match key.to_ascii_lowercase().as_str() {
                        "serveraliveinterval" => keepalive = milliseconds(value)?,
                        "connecttimeout" => timeout = milliseconds(value)?,
                        "stricthostkeychecking" => match value.to_ascii_lowercase().as_str() {
                            "no" | "off" | "false" => bypass = true,
                            "yes" | "ask" | "accept-new" | "true" => bypass = false,
                            _ => return Err("Invalid StrictHostKeyChecking value".into()),
                        },
                        "port" => {
                            if port.is_none() {
                                port = Some(parse_port(value)?);
                            }
                        }
                        "user" => {
                            if user.is_none() {
                                user = Some(value.into());
                            }
                        }
                        "identityfile" => {
                            if identity.is_none() {
                                identity = Some(value.into());
                            }
                        }
                        "proxyjump" => {
                            if proxy.is_none() {
                                proxy = Some(value.into());
                            }
                        }
                        "exitonforwardfailure" | "batchmode" => {
                            warnings.push("runtime_option_not_imported".into())
                        }
                        _ => return Err(format!("Unsupported SSH option: {key}")),
                    }
                }
                _ => unreachable!(),
            }
            break;
        }
        index += 1;
    }
    let mut target = endpoint(&target.ok_or("SSH destination is required")?)?;
    if let Some(port) = port {
        target.port = port;
    }
    if let Some(user) = user {
        target.user = user;
    }
    if let Some(identity) = identity {
        target.key_path = identity;
        target.auth_type = "ssh_key".into();
    }
    target.keep_alive_interval_ms = keepalive;
    target.timeout_ms = timeout;
    target.bypass_host_verification = bypass;
    let mut jumpers = Vec::new();
    if let Some(proxy) = proxy
        && !proxy.eq_ignore_ascii_case("none")
    {
        for hop in proxy.split(',') {
            jumpers.push(endpoint(hop)?);
        }
    }
    jumpers.push(target);
    if jumpers.iter().any(|jumper| jumper.user.is_empty()) {
        warnings.push("missing_user".into());
    }
    if bypass {
        warnings.push("host_verification_disabled".into());
    }
    let mut tunnels = Vec::new();
    for (mode, specification) in forwards {
        tunnels.push(forward(mode, &specification)?);
    }
    if tunnels.is_empty() {
        return Err("No TCP or SOCKS forwarding options found".into());
    }
    warnings.sort();
    warnings.dedup();
    Ok(CommandPreview {
        jumpers,
        tunnels,
        warnings,
    })
}

fn milliseconds(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_mul(1000))
        .ok_or("Invalid SSH interval".into())
}
fn parse_port(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Port must be between 1 and 65535".into())
}

fn endpoint(value: &str) -> Result<Jumper, String> {
    let value = value.strip_prefix("ssh://").unwrap_or(value);
    let (user, host_port) = value.rsplit_once('@').unwrap_or(("", value));
    let parts = colon_parts(host_port)?;
    if parts.is_empty()
        || parts.len() > 2
        || parts[0].is_empty()
        || parts[0].contains(['/', '\\', ' ', ';', '|', '&', '$', '`'])
    {
        return Err("Invalid SSH destination; bracket IPv6 addresses".into());
    }
    Ok(Jumper {
        name: parts[0].clone(),
        host: parts[0].clone(),
        user: user.into(),
        port: if parts.len() == 2 {
            parse_port(&parts[1])?
        } else {
            22
        },
        auth_type: "ssh_agent".into(),
        keep_alive_interval_ms: 5000,
        timeout_ms: 10000,
        ..Default::default()
    })
}

fn colon_parts(value: &str) -> Result<Vec<String>, String> {
    let mut parts = vec![String::new()];
    let mut bracket = false;
    for ch in value.chars() {
        match ch {
            '[' if !bracket && parts.last().unwrap().is_empty() => bracket = true,
            ']' if bracket => bracket = false,
            '[' | ']' => return Err("Invalid bracketed address".into()),
            ':' if !bracket => parts.push(String::new()),
            _ => parts.last_mut().unwrap().push(ch),
        }
    }
    if bracket {
        return Err("Unclosed bracketed address".into());
    }
    Ok(parts)
}

fn forward(mode: char, value: &str) -> Result<Tunnel, String> {
    let mut parts = colon_parts(value)?;
    let expected = if mode == 'D' { 2 } else { 4 };
    if parts.len() == expected - 1 {
        parts.insert(0, "127.0.0.1".into());
    }
    if parts.len() != expected {
        return Err("Only TCP port mappings and SOCKS listeners are supported".into());
    }
    if parts[0].is_empty() {
        parts[0] = "127.0.0.1".into();
    }
    if parts[0] == "*" {
        parts[0] = "0.0.0.0".into();
    }
    let port = parse_port(&parts[1])?;
    let mut tunnel = Tunnel {
        name: format!("{mode}-{port}"),
        reconnect_mode: "fixed".into(),
        ..Default::default()
    };
    match mode {
        'D' => {
            tunnel.mode = "dynamic".into();
            tunnel.local_host = parts[0].clone();
            tunnel.local_port = port;
        }
        'L' | 'R' => {
            if parts[2].is_empty() {
                return Err("Forward target host is required".into());
            }
            let target_port = parse_port(&parts[3])?;
            if mode == 'L' {
                tunnel.local_host = parts[0].clone();
                tunnel.local_port = port;
                tunnel.remote_host = parts[2].clone();
                tunnel.remote_port = target_port;
            } else {
                tunnel.mode = "remote".into();
                tunnel.remote_host = parts[0].clone();
                tunnel.remote_port = port;
                tunnel.local_host = parts[2].clone();
                tunnel.local_port = target_port;
            }
        }
        _ => unreachable!(),
    }
    Ok(tunnel)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_modes_attached_flags_quoted_key_and_chain() {
        let preview = parse(r#"ssh -NT -L8080:127.0.0.1:80 -R 0.0.0.0:9000:localhost:3000 -D1080 -D1081 -p2222 -i "C:\User Files\id_ed25519" -J ops@gateway:2200,relay@[::1] fixture@server"#).unwrap();
        assert_eq!(preview.tunnels.len(), 4);
        assert_eq!(preview.jumpers.len(), 3);
        assert_eq!(preview.jumpers[1].host, "::1");
        assert_eq!(preview.jumpers[2].port, 2222);
        assert_eq!(preview.jumpers[2].key_path, r"C:\User Files\id_ed25519");
        assert!(!preview.jumpers[2].bypass_host_verification);
        assert_eq!(preview.tunnels[1].remote_port, 9000);
        assert_eq!(preview.tunnels[1].local_port, 3000);
        assert_eq!(preview.tunnels[3].local_port, 1081);
    }
    #[test]
    fn bracketed_ipv6_and_explicit_options() {
        let preview = parse("ssh -L [::1]:8080:[2001:db8::1]:80 -o ServerAliveInterval=0 -o StrictHostKeyChecking=no -l fixture [::1]").unwrap();
        assert_eq!(preview.tunnels[0].local_host, "::1");
        assert_eq!(preview.tunnels[0].remote_host, "2001:db8::1");
        assert_eq!(preview.jumpers[0].keep_alive_interval_ms, 0);
        assert!(preview.jumpers[0].bypass_host_verification);
        let first = parse("ssh -D1080 -o StrictHostKeyChecking=yes -o StrictHostKeyChecking=no -o ConnectTimeout=8 -o ConnectTimeout=2 fixture@server").unwrap();
        assert!(!first.jumpers[0].bypass_host_verification);
        assert_eq!(first.jumpers[0].timeout_ms, 8000);
    }
    #[test]
    fn rejects_shell_remote_commands_unsupported_and_malformed_mappings() {
        for command in [
            "echo ssh -D1080 x@y",
            "ssh -D0 x@y",
            "ssh -L8080:/socket x@y",
            "ssh -D1080 x@y touch marker",
            "ssh -D1080 x@y;evil",
            "ssh -D1080 -o ProxyCommand=evil x@y",
            "ssh -i 'unterminated",
            "ssh -L8080:host:80bad x@y",
        ] {
            assert!(parse(command).is_err(), "{command}");
        }
        assert!(
            parse("ssh -D1080 alias")
                .unwrap()
                .warnings
                .contains(&"missing_user".into())
        );
    }
}
