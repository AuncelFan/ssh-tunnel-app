use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(anchor: &Path, args: &[&str], input: Option<Value>) -> Output {
    run_with(env!("CARGO_BIN_EXE_ssh-tunnel-cli"), anchor, args, input)
}
fn run_with(binary: &str, anchor: &Path, args: &[&str], input: Option<Value>) -> Output {
    let mut child = Command::new(binary)
        .args(args)
        .env("SSH_TUNNEL_CONFIG", anchor)
        .env(
            "SSH_TUNNEL_KNOWN_HOSTS",
            anchor.with_file_name("known_hosts"),
        )
        .env(
            "SSH_TUNNEL_AUTOSTART_NAME",
            format!("ssh-tunnel-test-cli-{}", std::process::id()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(value) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(value.to_string().as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}
fn call(anchor: &Path, command: &str, args: Value) -> Value {
    let result = run(anchor, &[command, "-"], Some(args));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("AI agent:"));
    serde_json::from_slice(&result.stdout).unwrap()
}
struct Service<'a>(&'a Path);
impl Drop for Service<'_> {
    fn drop(&mut self) {
        let _ = run(self.0, &["shutdown"], None);
    }
}

#[test]
fn agents_can_discover_commands_edit_redacted_records_and_confirm_exact_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let anchor = dir.path().join("config.toml");
    let other = dir.path().join("other.toml");
    let gui = env!("CARGO_BIN_EXE_ssh-tunnel");
    let version = env!("CARGO_PKG_VERSION");
    for (result, name) in [
        (run(&anchor, &["--version"], None), "ssh-tunnel-cli"),
        (run_with(gui, &anchor, &["--version"], None), "ssh-tunnel"),
    ] {
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            format!("{name} {version}")
        );
    }
    for arg in ["--help", "agent-help"] {
        let result = run(&anchor, &[arg], None);
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("AI agent guide"));
    }
    let help = run(&anchor, &["help", "clone_group"], None);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("target_jumper_id")
    );
    assert_eq!(run(&anchor, &["misspelled"], None).status.code(), Some(2));
    assert!(!anchor.exists());
    let _service = Service(&anchor);
    let _isolated = Service(&other);
    let mut jumper = json!({"name":"Fixture","host":"127.0.0.1","port":1,"user":"fixture","auth_type":"password","password":"synthetic-test-password"});
    let first = call(&anchor, "save_jumper", json!({"jumper":jumper}));
    jumper["name"] = json!("Target");
    let target = call(&anchor, "save_jumper", json!({"jumper":jumper}));
    let mut edited = call(&anchor, "list", json!({}))["jumpers"][0].clone();
    assert!(edited.get("password").is_none());
    edited["name"] = json!("Renamed");
    call(&anchor, "save_jumper", json!({"jumper":edited}));
    assert!(
        std::fs::read_to_string(&anchor)
            .unwrap()
            .matches("synthetic-test-password")
            .count()
            == 2
    );
    let source = call(
        &anchor,
        "save_tunnel",
        json!({"tunnel":{"name":"Web","jumper_ids":[first],"local_port":8080,"remote_port":80}}),
    );
    let clone = call(
        &anchor,
        "clone_group",
        json!({"request":{"tunnel_ids":[source],"target_jumper_id":target}}),
    );
    let state = call(&anchor, "list", json!({}));
    let legacy = run_with(gui, &anchor, &["cli", "--no-hints", "list"], None);
    assert!(legacy.status.success() && legacy.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&legacy.stdout).unwrap(),
        state
    );
    let quiet = run(&anchor, &["--no-hints", "status"], None);
    assert!(quiet.status.success() && quiet.stderr.is_empty());
    assert_eq!(state["groups"], json!([]));
    assert_eq!(state["tunnels"][1]["jumper_ids"], json!([target]));
    assert_eq!(call(&other, "list", json!({}))["tunnels"], json!([]));
    let rejected = run(
        &anchor,
        &["delete_selection", "-"],
        Some(json!({"ids":[],"jumperId":target})),
    );
    assert_eq!(rejected.status.code(), Some(1));
    call(
        &anchor,
        "delete_selection",
        json!({"ids":clone,"jumperId":target}),
    );
    assert_eq!(
        call(&anchor, "list", json!({}))["tunnels"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
