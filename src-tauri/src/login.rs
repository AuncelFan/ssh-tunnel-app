use ssh_tunnel_core::{controller::Controller, login::LoginStartup};

pub struct NativeLogin {
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    command: String,
    #[cfg(not(windows))]
    app: tauri::AppHandle,
}

pub fn entry_name() -> String {
    // Acceptance uses the shipping binary, but only in a reserved test namespace.
    if let Ok(name) = std::env::var("SSH_TUNNEL_AUTOSTART_NAME")
        && valid_test_name(&name)
    {
        return name;
    }
    crate::identity::CURRENT.startup_name.into()
}

fn valid_test_name(name: &str) -> bool {
    name.starts_with("ssh-tunnel-test-")
        && name.len() > "ssh-tunnel-test-".len()
        && name.len() <= 100
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(windows)]
fn command_line(executable: &str, config: &str) -> Result<String, String> {
    if [executable, config]
        .iter()
        .any(|s| s.contains(['"', '\0']) || s.is_empty())
    {
        return Err("Invalid startup executable or configuration path".into());
    }
    Ok(format!("\"{executable}\" --config \"{config}\""))
}

impl NativeLogin {
    pub fn new(_app: tauri::AppHandle, config: &std::path::Path) -> Result<Self, String> {
        #[cfg(windows)]
        {
            Ok(Self {
                name: entry_name(),
                command: command_line(
                    &std::env::current_exe()
                        .map_err(|_| "Cannot locate executable")?
                        .to_string_lossy(),
                    &config.to_string_lossy(),
                )?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Ok(Self { app: _app })
        }
    }
}

#[cfg(windows)]
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[cfg(windows)]
impl NativeLogin {
    fn system_allows(&self) -> Result<bool, String> {
        match windows_registry::CURRENT_USER
            .open(APPROVED)
            .and_then(|key| key.get_value(&self.name))
        {
            Ok(bytes) => Ok(!matches!(bytes.first(), Some(3 | 7))),
            Err(error) => {
                if matches!(error.code().0 as u32, 0x80070002 | 0x80070003) {
                    Ok(true)
                } else {
                    Err("Cannot read Windows startup approval".into())
                }
            }
        }
    }
}

#[cfg(windows)]
impl LoginStartup for NativeLogin {
    type Snapshot = Option<String>;

    fn snapshot(&self) -> Result<Self::Snapshot, String> {
        match windows_registry::CURRENT_USER
            .open(RUN)
            .and_then(|key| key.get_string(&self.name))
        {
            Ok(command) => Ok(Some(command)),
            Err(error) => {
                if matches!(error.code().0 as u32, 0x80070002 | 0x80070003) {
                    Ok(None)
                } else {
                    Err("Cannot read login startup registration".into())
                }
            }
        }
    }
    fn enabled(&self) -> Result<bool, String> {
        Ok(self.snapshot()?.as_ref() == Some(&self.command) && self.system_allows()?)
    }
    fn apply(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            if !self.system_allows()? {
                return Err(
                    "Windows has disabled this startup entry. Enable it in Task Manager first."
                        .into(),
                );
            }
            self.restore(Some(self.command.clone()))
        } else {
            self.restore(None)
        }
    }
    fn restore(&self, snapshot: Self::Snapshot) -> Result<(), String> {
        if let Some(command) = snapshot {
            windows_registry::CURRENT_USER
                .create(RUN)
                .and_then(|key| key.set_string(&self.name, &command))
                .map_err(|_| "Cannot register login startup".into())
        } else {
            match windows_registry::CURRENT_USER
                .options()
                .write()
                .open(RUN)
                .and_then(|key| key.remove_value(&self.name))
            {
                Ok(()) => Ok(()),
                Err(error) => {
                    if matches!(error.code().0 as u32, 0x80070002 | 0x80070003) {
                        Ok(())
                    } else {
                        Err("Cannot remove login startup registration".into())
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
impl LoginStartup for NativeLogin {
    type Snapshot = bool;
    fn snapshot(&self) -> Result<bool, String> {
        self.enabled()
    }
    fn restore(&self, snapshot: bool) -> Result<(), String> {
        self.apply(snapshot)
    }
    fn enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|e| e.to_string())
    }
    fn apply(&self, enabled: bool) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        let manager = self.app.autolaunch();
        (if enabled {
            manager.enable()
        } else {
            manager.disable()
        })
        .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn get_auto_run(state: tauri::State<'_, NativeLogin>) -> Result<bool, String> {
    state.enabled()
}

#[tauri::command]
pub async fn set_auto_run(
    controller: tauri::State<'_, Controller>,
    platform: tauri::State<'_, NativeLogin>,
    enabled: bool,
) -> Result<(), String> {
    let result = controller.set_auto_run(enabled, &*platform).await;
    controller.record_activity("set_auto_run", result.is_ok());
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_config_save_restores_exact_legacy_command_for_enable_and_disable() {
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Ok(key) = windows_registry::CURRENT_USER.options().write().open(RUN) {
                    let _ = key.remove_value(&self.0);
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let name = format!(
            "ssh-tunnel-test-rollback-{}-{}",
            std::process::id(),
            dir.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace('.', "")
        );
        let _cleanup = Cleanup(name.clone());
        let platform = NativeLogin {
            name,
            command: r#""C:\new\ssh-tunnel.exe" --config "C:\fixture\config.toml""#.into(),
        };
        let legacy = r#""C:\previous-location\ssh-tunnel.exe""#;
        let config = dir.path().join("config.toml");
        let controller = Controller::open(config.clone(), dir.path().join("known_hosts")).unwrap();
        std::fs::create_dir(&config).unwrap();
        for enabled in [true, false] {
            platform.restore(Some(legacy.into())).unwrap();
            assert!(!platform.enabled().unwrap());
            assert!(controller.set_auto_run(enabled, &platform).await.is_err());
            assert_eq!(platform.snapshot().unwrap().as_deref(), Some(legacy));
        }
        platform.restore(None).unwrap();
        assert!(controller.set_auto_run(true, &platform).await.is_err());
        assert_eq!(platform.snapshot().unwrap(), None);
    }
    #[test]
    fn acceptance_override_cannot_name_production_or_arbitrary_startup_entries() {
        assert!(valid_test_name("ssh-tunnel-test-release-123"));
        for name in [
            "ssh-tunnel",
            "ssh-tunnel-migration",
            "ssh-tunnel-test-",
            "ssh-tunnel-test-a/b",
            "ssh-tunnel-test-a b",
        ] {
            assert!(!valid_test_name(name));
        }
        assert!(!valid_test_name(&format!(
            "ssh-tunnel-test-{}",
            "a".repeat(100)
        )));
    }
    #[test]
    fn missing_registry_entry_means_disabled_not_an_error() {
        let name = format!(
            "ssh-tunnel-test-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let platform = NativeLogin {
            name,
            command: "unused".into(),
        };
        assert!(!platform.enabled().unwrap());
        assert!(platform.system_allows().unwrap());
        platform.apply(false).unwrap();
    }
    #[test]
    fn startup_paths_are_quoted_and_not_shell_interpolated() {
        assert_eq!(
            command_line(
                r"C:\Program Files\ssh-tunnel.exe",
                r"C:\User Data\config.toml"
            )
            .unwrap(),
            r#""C:\Program Files\ssh-tunnel.exe" --config "C:\User Data\config.toml""#
        );
        assert!(command_line("bad\"path", "config").is_err());
    }
}
