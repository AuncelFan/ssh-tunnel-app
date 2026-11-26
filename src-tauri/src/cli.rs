use crate::{config_path::configuration_path, ipc::Client};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub enum Entry {
    Console,
    Desktop,
}

impl Entry {
    fn backend_executable(&self, current: &Path) -> PathBuf {
        if matches!(self, Self::Desktop) {
            return current.to_owned();
        }
        // Portable downloads and staged builds can carry the version in both names.
        let suffix = std::env::consts::EXE_SUFFIX;
        let version = env!("CARGO_PKG_VERSION");
        for ending in [
            format!("-{version}-windows-x64{suffix}"),
            format!("-{version}{suffix}"),
        ] {
            if current.file_name().and_then(|s| s.to_str())
                == Some(&format!("ssh-tunnel-cli{ending}"))
            {
                return current.with_file_name(format!("ssh-tunnel{ending}"));
            }
        }
        current.with_file_name(format!("ssh-tunnel{suffix}"))
    }
}

pub fn arguments() -> Vec<String> {
    let mut arguments = std::env::args().skip(1);
    let mut positional = Vec::new();
    while let Some(arg) = arguments.next() {
        if arg == "--config" {
            arguments.next();
        } else {
            positional.push(arg);
        }
    }
    positional
}

const AGENT_GUIDE: &str = "AI agent guide\n  Discover the JSON arguments with `ssh-tunnel-cli help COMMAND`. Read `list` for current IDs and use the same configuration anchor as the GUI.\n  Supply UTF-8 JSON through stdin (-), especially for credentials. Parse stdout as JSON for operational commands and check the process exit code.\n  After start, poll list/status and get_pending_trust. Verify a reported fingerprint with the user or a trusted source before approving it.\n  Before deleting a jumper, review every tunnel referencing it and submit those exact IDs. Exit 3 contains a diagnostic report; inspect its checks and any host-key challenge.\n  Use --no-hints before a command to suppress routine stderr hints.";

fn agent_hint(command: &str) -> &'static str {
    match canonical(command) {
        "start_tunnel" => {
            "Poll list/status for the result; inspect get_pending_trust if approval is pending."
        }
        "test" | "run_probe" => {
            "Inspect the diagnostic checks and exit code. Verify the fingerprint before approving a key."
        }
        "get_pending_trust" => {
            "Verify the observed fingerprint before passing its tunnel ID and generation to resolve_host_key."
        }
        "list" => {
            "Use the returned IDs in later commands. Run `ssh-tunnel-cli help COMMAND` for its JSON arguments."
        }
        _ => {
            "Use `ssh-tunnel-cli agent-help` for automation guidance and `ssh-tunnel-cli help COMMAND` for arguments."
        }
    }
}

pub fn execute(args: &[String], entry: Entry) -> i32 {
    let quiet = args.first().is_some_and(|arg| arg == "--no-hints");
    let args = if quiet { &args[1..] } else { args };
    let command = args.first().map_or("help", String::as_str);
    if args.len() == 1 && matches!(command, "--version" | "-V") {
        let name = match entry {
            Entry::Desktop => "ssh-tunnel",
            Entry::Console => "ssh-tunnel-cli",
        };
        println!("{name} {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if args.len() == 1 && command == "agent-help" {
        println!("{AGENT_GUIDE}");
        return 0;
    }
    let result = run(args, entry);
    let code = result.unwrap_or_else(|(code, error)| {
        eprintln!("{error}");
        code
    });
    if !quiet && !matches!(command, "help" | "--help" | "-h") {
        eprintln!("AI agent: {}", agent_hint(command));
    }
    code
}

// Each entry is both discoverable in --help and callable by automation.
const COMMANDS: &[(&str, &str)] = &[
    (
        "list",
        "{}  Configuration and live status; jumper passwords omitted.",
    ),
    (
        "get_runtime",
        "{}  Runtime generations, errors, traffic and reconnect counters.",
    ),
    (
        "start_tunnel",
        r#"{"id":1}  Request startup; poll list and get_pending_trust."#,
    ),
    ("stop_tunnel", r#"{"id":1}  Stop and release the listener."#),
    (
        "test",
        r#"{"id":1}  Diagnose a saved tunnel; returns {token,report}. Stop its listener first."#,
    ),
    (
        "get_pending_trust",
        "{}  Observed runtime key challenges with tunnel_id and generation.",
    ),
    (
        "resolve_host_key",
        r#"{"id":1,"generation":1,"accept":false}  Verify the reported fingerprint independently before accepting."#,
    ),
    ("get_tunnel_logs", r#"{"id":1}  Tunnel event history."#),
    ("get_activity_logs", "{}  Application event history."),
    (
        "save_jumper",
        r#"{"jumper":{"name":"lab","host":"host.example","port":22,"user":"dev","auth_type":"ssh_agent"}}  Returns ID. id=0 creates; edits replace the record but preserve an omitted password. Set password to an empty string to clear it. auth_type: ssh_agent|ssh_key|password. Optional: password, key_path, agent_socket_path, timeout_ms, keep_alive_interval_ms, host_key_algorithms, notes, bypass_host_verification."#,
    ),
    (
        "save_tunnel",
        r#"{"tunnel":{"name":"Web","jumper_ids":[1],"mode":"local","local_port":8080,"remote_host":"127.0.0.1","remote_port":80}}  Returns ID. id=0 creates; edits replace the record. Modes: local|remote|dynamic. Optional: local_host, auto_start, description, reconnect_mode, reconnect_interval_seconds, reconnect_max_attempts."#,
    ),
    (
        "save_group",
        r#"{"group":{"id":0,"name":"Services","tunnel_ids":[1,2]}}  Create/update a custom group; returns ID."#,
    ),
    (
        "clone_group",
        r#"{"request":{"tunnel_ids":[1,2],"target_jumper_id":3}}  Clone chosen IDs to the target's default group; returns new IDs. Use group.tunnel_ids to clone a custom group."#,
    ),
    (
        "delete_selection",
        r#"{"ids":[1,2],"jumperId":null}  Delete selected tunnels. To delete a jumper too, supply its ID and exactly all tunnel IDs containing it in jumper_ids. Review list first."#,
    ),
    (
        "delete_jumper",
        r#"{"id":1}  Delete a jumper with no dependent tunnels."#,
    ),
    ("delete_tunnel", r#"{"id":1}  Stop and delete one tunnel."#),
    (
        "delete_group",
        r#"{"id":1}  Delete a custom group, retaining tunnels."#,
    ),
    (
        "reorder_groups",
        r#"{"ids":[2,1]}  All custom group IDs in display order."#,
    ),
    (
        "set_tunnel_auto_start",
        r#"{"request":{"id":1,"enabled":true}}  Change a tunnel's startup preference."#,
    ),
    (
        "begin_probe",
        r#"{"request":{"jumpers":[Jumper,...],"tunnel":Tunnel|null,"diagnostics":true}}  Begin testing drafts; returns token. Use save_jumper/save_tunnel shapes."#,
    ),
    (
        "run_probe",
        r#"{"token":1}  Run or rerun an existing test and return its report."#,
    ),
    (
        "approve_probe_key",
        r#"{"token":1}  Approve the observed test challenge after checking fingerprint, then run_probe again."#,
    ),
    (
        "cancel_probe",
        r#"{"token":1}  Cancel a test or reject its challenge."#,
    ),
    ("get_auto_run", "{}  Read OS login startup state."),
    (
        "set_auto_run",
        r#"{"enabled":false}  Update login startup."#,
    ),
    (
        "set_traffic_monitor",
        r#"{"enabled":true}  Toggle traffic collection."#,
    ),
    (
        "get_config_location",
        "{}  Show anchor, effective config and redirect file.",
    ),
    (
        "config_directory_conflicts",
        r#"{"target":null}  Check destination; null selects the default directory."#,
    ),
    (
        "relocate_config",
        r#"{"target":null,"overwrite":false}  Relocate to an absolute directory or restore default; returns warnings."#,
    ),
    (
        "preview_import",
        r#"{"path":"/absolute/config.toml"}  Preview replacement and receive a confirmation token."#,
    ),
    (
        "confirm_import",
        r#"{"token":1}  Apply preview and restart configured tunnels; returns warnings."#,
    ),
    ("cancel_import", "{}  Discard preview."),
    (
        "export_config",
        r#"{"path":"/absolute/export.toml"}  Export complete config including plaintext credentials; protect the destination."#,
    ),
    (
        "load_ssh_config",
        r#"{"path":"/absolute/ssh/config"}  Preview SSH host aliases."#,
    ),
    (
        "import_jumpers",
        r#"{"jumpers":[Jumper,...]}  Import selected candidates."#,
    ),
    (
        "parse_ssh_command",
        r#"{"command":"ssh -L 8080:localhost:80 dev@host.example"}  Preview jumpers and tunnels."#,
    ),
    (
        "import_command_tunnels",
        r#"{"request":{"jumpers":[Jumper,...],"tunnels":[Tunnel,...]}}  Import selected preview records."#,
    ),
    ("take_startup_errors", "{}  Consume startup warnings."),
    (
        "shutdown",
        "{}  Stop tunnels and exit the background process.",
    ),
];
fn help() {
    println!(
        "ssh-tunnel-cli {}\nUsage: ssh-tunnel-cli [--config ABSOLUTE_PATH] [--no-hints] COMMAND [ID | JSON | -]\nUse ssh-tunnel for the GUI. The compatibility form is ssh-tunnel cli COMMAND. GUI and CLI use the same per-user backend for this configuration anchor. SSH_TUNNEL_CONFIG takes precedence over --config. Commands start the backend on demand; closing the CLI leaves tunnels running. Quit application or shutdown stops them.\n\nExamples (PowerShell):\n  .\\ssh-tunnel-cli.exe list | ConvertFrom-Json\n  .\\ssh-tunnel-cli.exe start 1\n  .\\ssh-tunnel-cli.exe status | ConvertFrom-Json\n  Get-Content -Raw request.json | .\\ssh-tunnel-cli.exe clone_group -\n\nPass UTF-8 JSON via stdin (-) for credentials. Successful commands write one JSON value to stdout. Errors go to stderr. Exit 0 = success, 1 = operation failed, 2 = invalid command/input, 3 = completed diagnostic failed or needs key approval (see JSON). IDs in examples are illustrative. Aliases: status=get_runtime, start=start_tunnel, stop=stop_tunnel, logs=get_tunnel_logs. Use help COMMAND for argument details, agent-help for automation guidance, and --version for the application version.\n\n{AGENT_GUIDE}\n",
        env!("CARGO_PKG_VERSION")
    );
    for (name, usage) in COMMANDS {
        println!("  {name:26} {usage}");
    }
}
fn canonical(name: &str) -> &str {
    match name {
        "start" => "start_tunnel",
        "stop" => "stop_tunnel",
        "logs" => "get_tunnel_logs",
        "status" => "get_runtime",
        other => other,
    }
}
fn run(args: &[String], entry: Entry) -> Result<i32, (i32, String)> {
    let name = args.first().map(String::as_str).unwrap_or("help");
    if matches!(name, "help" | "--help" | "-h") && args.len() == 1 || args.is_empty() {
        help();
        return Ok(0);
    }
    let topic = if name == "help" {
        args.get(1).map(String::as_str).unwrap_or("")
    } else {
        name
    };
    let command = canonical(topic);
    let (_, usage) = COMMANDS
        .iter()
        .find(|(key, _)| *key == command)
        .ok_or_else(|| {
            (
                2,
                format!("Unknown command {topic}. See ssh-tunnel-cli --help."),
            )
        })?;
    if name == "help" || args.get(1).is_some_and(|s| s == "--help" || s == "-h") {
        println!("{command} {usage}");
        return Ok(0);
    }
    if args.len() > 2 {
        return Err((2, "Expected one JSON argument or - for stdin".into()));
    }
    let input = match args.get(1).map(String::as_str) {
        None => json!({}),
        Some(raw)
            if [
                "start_tunnel",
                "stop_tunnel",
                "test",
                "get_tunnel_logs",
                "delete_tunnel",
                "delete_jumper",
                "delete_group",
            ]
            .contains(&command)
                && raw.parse::<u64>().is_ok() =>
        {
            json!({"id":raw.parse::<u64>().unwrap()})
        }
        Some(raw) => {
            let mut text = String::new();
            let raw = if raw == "-" {
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| (2, e.to_string()))?;
                text.trim_start_matches('\u{feff}')
            } else {
                raw
            };
            serde_json::from_str(raw).map_err(|e| (2, format!("Invalid JSON: {e}")))?
        }
    };
    if !input.is_object() {
        return Err((2, "Expected a JSON object".into()));
    }
    let anchor = configuration_path().map_err(|e| (2, e))?;
    let client = Client::new(&anchor).map_err(|e| (1, e.to_string()))?;
    if command != "shutdown" {
        let current = std::env::current_exe().map_err(|e| (1, e.to_string()))?;
        client
            .ensure(&anchor, &entry.backend_executable(&current))
            .map_err(|e| (1, e.to_string()))?;
    }
    let result = client.call(command, input).map_err(|e| (1, e))?;
    println!("{result}");
    let report = match command {
        "test" => result.get("report"),
        "run_probe" => Some(&result),
        _ => None,
    };
    Ok(
        if report.is_some_and(|r| r["success"] == Value::Bool(false)) {
            3
        } else {
            0
        },
    )
}
