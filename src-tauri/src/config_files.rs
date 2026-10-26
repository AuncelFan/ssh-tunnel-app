use ssh_tunnel_core::controller::{Controller, ImportPreview};
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;

fn home_directory() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("Cannot locate home directory".into())
}

#[tauri::command]
pub fn default_ssh_config_path() -> Result<String, String> {
    Ok(home_directory()?
        .join(".ssh")
        .join("config")
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn load_ssh_config(
    path: PathBuf,
) -> Result<ssh_tunnel_core::ssh_config::Preview, String> {
    let home = home_directory()?;
    tauri::async_runtime::spawn_blocking(move || ssh_tunnel_core::ssh_config::load(&path, &home))
        .await
        .map_err(|_| "SSH config import failed".to_string())?
}

#[tauri::command]
pub fn get_config_path(state: tauri::State<'_, Controller>) -> String {
    state.config_path().to_string_lossy().into_owned()
}

#[tauri::command]
pub fn get_config_location(
    state: tauri::State<'_, Controller>,
) -> ssh_tunnel_core::location::Location {
    state.config_location()
}

#[tauri::command]
pub fn config_directory_conflicts(
    state: tauri::State<'_, Controller>,
    target: Option<PathBuf>,
) -> Result<ssh_tunnel_core::location::Conflict, String> {
    let location = state.config_location();
    let target = target.unwrap_or_else(|| location.implicit.parent().unwrap().to_owned());
    ssh_tunnel_core::location::conflicts(&location, &target).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn relocate_config(
    state: tauri::State<'_, Controller>,
    desktop: tauri::State<'_, crate::desktop::Desktop>,
    platform: tauri::State<'_, crate::login::NativeLogin>,
    target: Option<PathBuf>,
    overwrite: bool,
) -> Result<Vec<String>, String> {
    let result = state.relocate_config(target, overwrite).await;
    state.record_activity("relocate_config", result.is_ok());
    let mut warnings = result?;
    if let Err(error) = crate::desktop::reload_locale(&desktop, &state.config_path()) {
        warnings.push(error);
    }
    if let Err(error) = state.reconcile_auto_run(&*platform).await {
        warnings.push(error);
    }
    Ok(warnings)
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
pub fn reveal_config_directory(state: tauri::State<'_, Controller>) -> Result<(), String> {
    let config = state.config_path();
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

#[tauri::command]
pub async fn preview_import(
    state: tauri::State<'_, Controller>,
    path: PathBuf,
) -> Result<ImportPreview, String> {
    state.preview_import(path).await
}

#[tauri::command]
pub async fn confirm_import(
    state: tauri::State<'_, Controller>,
    platform: tauri::State<'_, crate::login::NativeLogin>,
    token: u64,
) -> Result<Vec<String>, String> {
    let result = state.confirm_import(token).await;
    state.record_activity("confirm_import", result.is_ok());
    let mut warnings = result?;
    if let Err(error) = state.reconcile_auto_run(&*platform).await {
        warnings.push(error);
    }
    Ok(warnings)
}

#[tauri::command]
pub async fn cancel_import(state: tauri::State<'_, Controller>) -> Result<(), String> {
    state.cancel_import().await;
    Ok(())
}

#[tauri::command]
pub async fn export_config(
    state: tauri::State<'_, Controller>,
    path: PathBuf,
) -> Result<(), String> {
    let result = state.export_config(path).await;
    state.record_activity("export_config", result.is_ok());
    result
}
