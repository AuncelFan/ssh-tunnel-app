use crate::model::Jumper;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize)]
pub struct Candidate {
    pub alias: String,
    pub jumper: Jumper,
    pub proxy_jump: String,
    pub warnings: Vec<String>,
}

struct Block {
    patterns: Vec<String>,
    guards: Vec<Vec<String>>,
    options: Vec<(String, String)>,
    conditional: bool,
}
struct Parser {
    blocks: Vec<Block>,
    stack: HashSet<PathBuf>,
    home: PathBuf,
    bytes: usize,
    files: usize,
    warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct Preview {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}

pub fn load(path: &Path, home: &Path) -> Result<Preview, String> {
    if !path.is_absolute() || !home.is_absolute() {
        return Err("SSH config paths must be absolute".into());
    }
    let mut parser = Parser {
        blocks: vec![Block {
            patterns: vec!["*".into()],
            guards: vec![],
            options: vec![],
            conditional: false,
        }],
        stack: HashSet::new(),
        home: home.to_owned(),
        bytes: 0,
        files: 0,
        warnings: vec![],
    };
    parser.file(path, vec![])?;
    let mut aliases = Vec::new();
    let mut seen = HashSet::new();
    for block in &parser.blocks {
        for pattern in &block.patterns {
            if !pattern.starts_with('!')
                && !pattern.contains(['*', '?'])
                && block
                    .guards
                    .iter()
                    .all(|guard| host_matches(guard, pattern))
                && seen.insert(pattern.to_lowercase())
            {
                aliases.push(pattern.clone());
            }
        }
    }
    let candidates = aliases
        .iter()
        .map(|alias| parser.resolve(alias))
        .collect::<Result<_, _>>()?;
    parser.warnings.sort();
    parser.warnings.dedup();
    Ok(Preview {
        candidates,
        warnings: parser.warnings,
    })
}

impl Parser {
    fn file(&mut self, path: &Path, guards: Vec<Vec<String>>) -> Result<(), String> {
        if self.stack.len() >= 16 || self.files >= 128 {
            return Err("SSH config include limit exceeded".into());
        }
        let canonical =
            std::fs::canonicalize(path).map_err(|_| "Cannot open SSH config or included file")?;
        if !self.stack.insert(canonical.clone()) {
            return Err("SSH config Include cycle detected".into());
        }
        self.files += 1;
        let length = std::fs::metadata(path)
            .map_err(|_| "Cannot read SSH config metadata")?
            .len();
        if length > 2_000_000 || self.bytes as u64 + length > 8_000_000 {
            return Err("SSH config size limit exceeded".into());
        }
        let text = std::fs::read_to_string(path).map_err(|_| "Cannot read SSH config text")?;
        self.bytes += text.len();
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let split = line
                .find(|c: char| c.is_whitespace() || c == '=')
                .unwrap_or(line.len());
            let key = line[..split].to_ascii_lowercase();
            let values = words(
                line[split..]
                    .trim_start()
                    .trim_start_matches('=')
                    .trim_start(),
            )?;
            if values.is_empty() {
                return Err("SSH config directive has no value".into());
            }
            match key.as_str() {
                "host" => self.blocks.push(Block {
                    patterns: values,
                    guards: guards.clone(),
                    options: vec![],
                    conditional: false,
                }),
                "match" => {
                    self.warnings.push("match_unsupported".into());
                    self.blocks.push(Block {
                        patterns: vec![],
                        guards: guards.clone(),
                        options: vec![],
                        conditional: true,
                    });
                }
                "include" => {
                    if self.blocks.last().unwrap().conditional {
                        continue;
                    }
                    let scope = self.blocks.last().unwrap();
                    let parent_patterns = scope.patterns.clone();
                    let parent_guards = scope.guards.clone();
                    let mut inherited = parent_guards.clone();
                    inherited.push(parent_patterns.clone());
                    for value in values {
                        let expanded = expand_path(&value, &self.home, path.parent().unwrap())?;
                        let mut matches = glob::glob(&expanded.to_string_lossy())
                            .map_err(|_| "Invalid SSH Include pattern")?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| "Cannot enumerate SSH Include pattern")?;
                        matches.sort();
                        for included in matches {
                            if included.is_file() {
                                self.blocks.push(Block {
                                    patterns: vec!["*".into()],
                                    guards: inherited.clone(),
                                    options: vec![],
                                    conditional: false,
                                });
                                self.file(&included, inherited.clone())?;
                                // OpenSSH restores the enclosing Host context after each Include.
                                self.blocks.push(Block {
                                    patterns: parent_patterns.clone(),
                                    guards: parent_guards.clone(),
                                    options: vec![],
                                    conditional: false,
                                });
                            }
                        }
                    }
                }
                _ => self
                    .blocks
                    .last_mut()
                    .unwrap()
                    .options
                    .push((key, values.join(" "))),
            }
        }
        self.stack.remove(&canonical);
        Ok(())
    }

    fn resolve(&self, alias: &str) -> Result<Candidate, String> {
        let mut options = HashMap::new();
        let mut warnings = Vec::new();
        let mut identities = 0;
        for block in &self.blocks {
            if block.conditional
                || !host_matches(&block.patterns, alias)
                || !block.guards.iter().all(|guard| host_matches(guard, alias))
            {
                continue;
            }
            for (key, value) in &block.options {
                if key == "identityfile" {
                    identities += 1;
                }
                // OpenSSH uses the first obtained scalar value, even if it equals the default.
                options.entry(key.as_str()).or_insert(value.as_str());
            }
        }
        let get = |key| options.get(key).copied().unwrap_or("");
        let number = |key, default| -> Result<u64, String> {
            if get(key).is_empty() {
                return Ok(default);
            }
            get(key)
                .parse::<u64>()
                .map_err(|_| format!("Invalid numeric SSH option: {key}"))
        };
        let port = number("port", 22)?;
        if port == 0 || port > 65535 {
            return Err("SSH port is outside 1..65535".into());
        }
        let host = if get("hostname").is_empty() {
            alias.to_owned()
        } else {
            expand_tokens(get("hostname"), alias, alias, get("user"), port, &self.home)?
        };
        let user = get("user").to_owned();
        let raw_key = get("identityfile");
        let key_path = if raw_key.is_empty() || raw_key.eq_ignore_ascii_case("none") {
            String::new()
        } else {
            let expanded = expand_tokens(raw_key, alias, &host, &user, port, &self.home)?;
            expand_path(&expanded, &self.home, &self.home)?
                .to_string_lossy()
                .into_owned()
        };
        let raw_agent = get("identityagent");
        let agent_socket_path =
            if raw_agent.is_empty() || raw_agent.eq_ignore_ascii_case("SSH_AUTH_SOCK") {
                String::new()
            } else if raw_agent.eq_ignore_ascii_case("none") {
                warnings.push("agent_disabled".into());
                String::new()
            } else {
                let expanded = expand_tokens(raw_agent, alias, &host, &user, port, &self.home)?;
                expand_path(&expanded, &self.home, &self.home)?
                    .to_string_lossy()
                    .into_owned()
            };
        let proxy_jump = match get("proxyjump") {
            "" | "none" => String::new(),
            value => value.into(),
        };
        if !proxy_jump.is_empty() {
            warnings.push("proxy_jump".into());
        }
        if !get("proxycommand").is_empty() && get("proxycommand") != "none" {
            warnings.push("proxy_command_unsupported".into());
        }
        if !get("userknownhostsfile").is_empty() || !get("hostkeyalias").is_empty() {
            warnings.push("host_key_options_unsupported".into());
        }
        if identities > 1 {
            warnings.push("multiple_identity_files".into());
        }
        if user.is_empty() {
            warnings.push("missing_user".into());
        }
        if key_path.is_empty() {
            warnings.push("agent_fallback".into());
        }
        let bypass = matches!(
            get("stricthostkeychecking").to_ascii_lowercase().as_str(),
            "no" | "off" | "false"
        );
        if bypass {
            warnings.push("host_verification_disabled".into());
        }
        let interval = number("serveraliveinterval", 5)?
            .checked_mul(1000)
            .ok_or("SSH keepalive interval overflow")?;
        let timeout = number("connecttimeout", 5)?
            .checked_mul(1000)
            .ok_or("SSH timeout overflow")?;
        Ok(Candidate {
            alias: alias.into(),
            proxy_jump,
            warnings,
            jumper: Jumper {
                name: alias.into(),
                host,
                user,
                port: port as u16,
                auth_type: if key_path.is_empty() {
                    "ssh_agent"
                } else {
                    "ssh_key"
                }
                .into(),
                key_path,
                agent_socket_path,
                bypass_host_verification: bypass,
                keep_alive_interval_ms: interval,
                timeout_ms: timeout,
                host_key_algorithms: get("hostkeyalgorithms").into(),
                ..Default::default()
            },
        })
    }
}

pub(crate) fn words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '#' if quote.is_none() => break,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '\\' if chars.peek().is_some_and(|next| {
                *next == '"'
                    || *next == '\''
                    || *next == '#'
                    || (quote.is_none() && next.is_whitespace())
            }) =>
            {
                word.push(chars.next().unwrap())
            }
            ch if ch.is_whitespace() && quote.is_none() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(ch),
        }
    }
    if quote.is_some() {
        return Err("Unterminated quote in SSH config".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn host_matches(patterns: &[String], alias: &str) -> bool {
    let mut positive = false;
    for pattern in patterns {
        let (negated, raw) = pattern
            .strip_prefix('!')
            .map_or((false, pattern.as_str()), |raw| (true, raw));
        let matched = crate::hostkeys::wildcard(&raw.to_lowercase(), &alias.to_lowercase());
        if matched && negated {
            return false;
        }
        positive |= matched;
    }
    positive
}

fn expand_path(raw: &str, home: &Path, base: &Path) -> Result<PathBuf, String> {
    let expanded = expand_environment(raw, home)?;
    let raw = expanded.as_str();
    let path = if raw == "~" {
        home.to_owned()
    } else if raw.starts_with("~/") || raw.starts_with("~\\") {
        home.join(&raw[2..])
    } else if raw.starts_with('~') {
        return Err("Named-user home paths are not supported".into());
    } else {
        PathBuf::from(raw)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

fn expand_environment(raw: &str, home: &Path) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }
        let mut name = String::new();
        if chars.peek() == Some(&'{') {
            chars.next();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => name.push(ch),
                    _ => return Err("Invalid environment variable in SSH path".into()),
                }
            }
        } else {
            while chars
                .peek()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            {
                name.push(chars.next().unwrap());
            }
        }
        if name.is_empty() {
            return Err("Invalid environment variable in SSH path".into());
        }
        if matches!(name.as_str(), "HOME" | "USERPROFILE") {
            result.push_str(&home.to_string_lossy());
        } else {
            result.push_str(
                &std::env::var(name).map_err(|_| "Missing environment variable in SSH path")?,
            );
        }
    }
    Ok(result)
}

fn expand_tokens(
    raw: &str,
    alias: &str,
    host: &str,
    user: &str,
    port: u64,
    home: &Path,
) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        match chars.next() {
            Some('%') => result.push('%'),
            Some('n') => result.push_str(alias),
            Some('h') => result.push_str(host),
            Some('r') => result.push_str(user),
            Some('p') => result.push_str(&port.to_string()),
            Some('d') => result.push_str(&home.to_string_lossy()),
            _ => return Err("Unsupported token in SSH config path or hostname".into()),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_include_preserves_guards_and_restores_parent_scope() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::write(
            dir.path().join("child"),
            "User included\nHost other\n User nested-other\n",
        )
        .unwrap();
        std::fs::write(
            &root,
            "Host outer\n Include child\n Port 2222\nHost other\n User actual\n",
        )
        .unwrap();
        let preview = load(&root, dir.path()).unwrap();
        assert_eq!(preview.candidates.len(), 2);
        assert_eq!(preview.candidates[0].jumper.user, "included");
        assert_eq!(preview.candidates[0].jumper.port, 2222);
        assert_eq!(preview.candidates[1].jumper.user, "actual");
        assert_eq!(preview.candidates[1].jumper.port, 22);
        assert!(preview.warnings.is_empty());
    }
    #[test]
    fn conditional_include_negation_and_repeated_include_are_not_global() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::write(dir.path().join("child"), "User included\n").unwrap();
        std::fs::write(&root, "Host *.corp !excluded.corp\n Include child\nHost remote\n Include child\nHost good.corp excluded.corp remote\n User fallback\n").unwrap();
        let preview = load(&root, dir.path()).unwrap();
        let users: HashMap<_, _> = preview
            .candidates
            .iter()
            .map(|c| (c.alias.as_str(), c.jumper.user.as_str()))
            .collect();
        assert_eq!(users["good.corp"], "included");
        assert_eq!(users["excluded.corp"], "fallback");
        assert_eq!(users["remote"], "included");
    }
    #[test]
    fn includes_quotes_tokens_and_first_obtained_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::write(dir.path().join("hosts included"), "Host target alias\n HostName internal.example\n User deploy\n Port 2200\n IdentityFile \"%d/.ssh/key # %n\"\n ProxyJump gateway\n").unwrap();
        std::fs::write(&root, "User root\nPort 22\nInclude \"hosts included\"\nHost *\n ServerAliveInterval 0\n ConnectTimeout 8\n").unwrap();
        let preview = load(&root, dir.path()).unwrap();
        assert_eq!(preview.candidates.len(), 2);
        let first = &preview.candidates[0];
        assert_eq!(first.jumper.host, "internal.example");
        assert_eq!(first.jumper.user, "root");
        assert_eq!(first.jumper.port, 22);
        assert_eq!(first.jumper.keep_alive_interval_ms, 0);
        assert_eq!(first.jumper.timeout_ms, 8000);
        assert_eq!(first.proxy_jump, "gateway");
        assert!(first.jumper.key_path.ends_with("key # target"));
        assert!(first.warnings.contains(&"proxy_jump".into()));
    }
    #[test]
    fn patterns_negation_match_and_commands_do_not_leak_options() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::write(&root, "Host * !private\n User shared\nHost public private\n HostName %n.example\nMatch exec \"never run me\"\n User wrong\n Port 9999\nHost private\n User owner\n ProxyCommand never-run\n StrictHostKeyChecking no\n").unwrap();
        let preview = load(&root, dir.path()).unwrap();
        assert!(preview.warnings.contains(&"match_unsupported".into()));
        assert_eq!(preview.candidates[0].jumper.user, "shared");
        assert_eq!(preview.candidates[1].jumper.user, "owner");
        assert_eq!(preview.candidates[1].jumper.port, 22);
        assert!(preview.candidates[1].jumper.bypass_host_verification);
        assert!(
            preview.candidates[1]
                .warnings
                .contains(&"proxy_command_unsupported".into())
        );
    }
    #[test]
    fn windows_paths_environment_and_quoted_comments() {
        assert_eq!(
            words(r#""C:\Users\User Name\.ssh\id" # note"#).unwrap(),
            vec![r"C:\Users\User Name\.ssh\id"]
        );
        let home = std::env::temp_dir();
        assert_eq!(
            expand_path("${HOME}/.ssh/key", &home, &home).unwrap(),
            home.join(".ssh/key")
        );
        assert!(words("'unterminated").is_err());
    }
    #[test]
    fn include_cycles_and_invalid_ports_fail_without_disclosing_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::write(&root, "Include config\n").unwrap();
        assert!(load(&root, dir.path()).err().unwrap().contains("cycle"));
        std::fs::write(&root, "Host test\nPort secret-invalid\n").unwrap();
        let error = load(&root, dir.path()).err().unwrap();
        assert!(!error.contains("secret-invalid"));
        assert!(load(&dir.path().join("absent"), dir.path()).is_err());
    }
}
