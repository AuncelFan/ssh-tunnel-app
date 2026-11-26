use ssh_tunnel::Client;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub fn default_ssh_config_path() -> Result<String, String> {
    Ok(ssh_tunnel::home()?
        .join(".ssh")
        .join("config")
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn choose_config_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|_| "A local directory is required".into())
            })
            .transpose()
    })
    .await
    .map_err(|_| "Directory dialog failed".to_string())?
}

#[tauri::command]
pub fn reveal_config_directory(state: tauri::State<'_, Client>) -> Result<(), String> {
    let config = state.config_path()?;
    let directory = config.parent().ok_or("Missing configuration directory")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("explorer.exe");
        command.creation_flags(0x08000000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(directory)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn choose_config_file(
    app: tauri::AppHandle,
    save: bool,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().add_filter("TOML", &["toml"]);
        let selected = if save {
            dialog.set_file_name("config.toml").blocking_save_file()
        } else {
            dialog.blocking_pick_file()
        };
        selected
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|_| "A local file is required".into())
            })
            .transpose()
    })
    .await
    .map_err(|_| "File dialog failed".to_string())?
}
