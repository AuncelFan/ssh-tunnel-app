use crate::login::NativeLogin;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use ssh_tunnel_core::{controller::Controller, login::LoginStartup, probe_session};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Backend {
    pub controller: Controller,
    login: NativeLogin,
    startup_errors: Mutex<Vec<String>>,
}

pub fn home() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("Cannot locate home directory".into())
}

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let value = args
        .get(key)
        .ok_or_else(|| format!("Missing argument {key}"))?;
    serde_json::from_value(value.clone()).map_err(|e| format!("Invalid {key}: {e}"))
}
fn output(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}
impl Backend {
    pub async fn open(path: &Path) -> Result<Self, String> {
        let known = match std::env::var_os("SSH_TUNNEL_KNOWN_HOSTS") {
            Some(path) => PathBuf::from(path),
            None => home()?.join(".ssh/known_hosts"),
        };
        if !known.is_absolute() {
            return Err("SSH_TUNNEL_KNOWN_HOSTS must be absolute".into());
        }
        let controller = Controller::open(path.to_owned(), known).map_err(|e| e.to_string())?;
        let login = NativeLogin::new(path)?;
        let mut errors = controller.start_automatically().await;
        if let Err(error) = controller.reconcile_auto_run(&login).await {
            errors.push(error);
        }
        Ok(Self {
            controller,
            login,
            startup_errors: Mutex::new(errors),
        })
    }
    pub async fn execute(&self, command: &str, args: Value) -> Result<Value, String> {
        let c = &self.controller;
        macro_rules! change {
            ($call:expr) => {{
                let result = $call.await;
                c.record_activity(command, result.is_ok());
                output(result?)
            }};
        }
        match command {
            "get_state" => output(c.snapshot()),
            "list" => {
                let mut config = output(c.snapshot())?;
                for jumper in config["jumpers"].as_array_mut().unwrap() {
                    jumper.as_object_mut().unwrap().remove("password");
                }
                Ok(config)
            }
            "get_runtime" => output(c.runtime_state()),
            "get_tunnel_logs" => output(c.logs(arg(&args, "id")?)),
            "get_activity_logs" => output(c.activity_logs()),
            "get_pending_trust" => output(c.pending_trust()),
            "resolve_host_key" => output(
                c.resolve_host_key(
                    arg(&args, "id")?,
                    arg(&args, "generation")?,
                    arg(&args, "accept")?,
                )
                .await?,
            ),
            "start_tunnel" => output(c.start(arg(&args, "id")?).await?),
            "stop_tunnel" => {
                let id: u64 = arg(&args, "id")?;
                if !c.snapshot().tunnels.iter().any(|t| t.id == id) {
                    return Err("Tunnel not found".into());
                }
                c.stop(id).await;
                Ok(Value::Null)
            }
            "save_jumper" => change!(c.save_jumper_record(
                arg(&args, "jumper")?,
                args["jumper"].get("password").is_none()
            )),
            "save_tunnel" => change!(c.save_tunnel(arg(&args, "tunnel")?)),
            "save_group" => change!(c.save_group(arg(&args, "group")?)),
            "delete_jumper" => change!(c.delete_jumper(arg(&args, "id")?)),
            "delete_tunnel" => change!(c.delete_tunnel(arg(&args, "id")?)),
            "delete_group" => change!(c.delete_group(arg(&args, "id")?)),
            "delete_selection" => change!(
                c.delete_selection(&arg::<Vec<u64>>(&args, "ids")?, arg(&args, "jumperId")?)
            ),
            "clone_group" => change!(c.clone_group(arg(&args, "request")?)),
            "reorder_groups" => change!(c.reorder_groups(arg(&args, "ids")?)),
            "set_tunnel_auto_start" => change!(c.set_tunnel_auto_start(arg(&args, "request")?)),
            "set_traffic_monitor" => change!(c.set_traffic_monitor(arg(&args, "enabled")?)),
            "get_auto_run" => output(self.login.enabled()?),
            "set_auto_run" => change!(c.set_auto_run(arg(&args, "enabled")?, &self.login)),
            "get_config_path" => output(c.config_path()),
            "get_config_location" => output(c.config_location()),
            "config_directory_conflicts" => {
                let location = c.config_location();
                let target = arg::<Option<PathBuf>>(&args, "target")?
                    .unwrap_or_else(|| location.implicit.parent().unwrap().into());
                output(
                    ssh_tunnel_core::location::conflicts(&location, &target)
                        .map_err(|e| e.to_string())?,
                )
            }
            "relocate_config" | "confirm_import" => {
                let result = if command == "relocate_config" {
                    c.relocate_config(arg(&args, "target")?, arg(&args, "overwrite")?)
                        .await
                } else {
                    c.confirm_import(arg(&args, "token")?).await
                };
                c.record_activity(command, result.is_ok());
                let mut warnings = result?;
                if let Err(e) = c.reconcile_auto_run(&self.login).await {
                    warnings.push(e);
                }
                output(warnings)
            }
            "take_startup_errors" => {
                output(std::mem::take(&mut *self.startup_errors.lock().unwrap()))
            }
            "preview_import" => output(c.preview_import(arg(&args, "path")?).await?),
            "cancel_import" => {
                c.cancel_import().await;
                Ok(Value::Null)
            }
            "export_config" => change!(c.export_config(arg(&args, "path")?)),
            "parse_ssh_command" => output(ssh_tunnel_core::ssh_command::parse(&arg::<String>(
                &args, "command",
            )?)?),
            "load_ssh_config" => output(ssh_tunnel_core::ssh_config::load(
                &arg::<PathBuf>(&args, "path")?,
                &home()?,
            )?),
            "import_jumpers" => change!(c.import_jumpers(arg(&args, "jumpers")?)),
            "import_command_tunnels" => change!(c.import_command_tunnels(arg(&args, "request")?)),
            "begin_probe" => output(c.begin_probe(arg(&args, "request")?).await?),
            "approve_probe_key" => change!(c.approve_probe_key(arg(&args, "token")?)),
            "run_probe" => output(c.run_probe(arg(&args, "token")?).await?),
            "cancel_probe" => {
                c.cancel_probe(arg(&args, "token")?).await;
                Ok(Value::Null)
            }
            "test" => {
                let id: u64 = arg(&args, "id")?;
                let config = c.snapshot();
                let tunnel = config
                    .tunnels
                    .into_iter()
                    .find(|t| t.id == id)
                    .ok_or("Tunnel not found")?;
                let jumpers = tunnel
                    .jumper_ids
                    .iter()
                    .map(|id| {
                        config
                            .jumpers
                            .iter()
                            .find(|j| j.id == *id)
                            .cloned()
                            .ok_or("Missing jumper")
                    })
                    .collect::<Result<_, _>>()?;
                let token = c
                    .begin_probe(probe_session::Request {
                        jumpers,
                        tunnel: Some(tunnel),
                        diagnostics: true,
                    })
                    .await?;
                Ok(json!({"token":token,"report":c.run_probe(token).await?}))
            }
            _ => Err(format!("Unknown backend command: {command}")),
        }
    }
}
