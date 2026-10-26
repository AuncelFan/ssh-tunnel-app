#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_files;
mod config_path;
mod desktop;
mod identity;
mod login;
mod startup_trace;

use config_path::configuration_path;

use ssh_tunnel_core::{
    controller::Controller,
    model::{Config, Group, Jumper, Tunnel},
    runtime::{RuntimeEvent, TunnelSnapshot},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tauri::{Emitter, Manager};

#[derive(Default)]
struct StartupErrors(std::sync::Mutex<Vec<String>>);

#[tauri::command]
fn take_startup_errors(state: tauri::State<'_, StartupErrors>) -> Vec<String> {
    std::mem::take(&mut *state.0.lock().expect("Startup errors lock poisoned"))
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn get_state(state: tauri::State<'_, Controller>) -> Config {
    state.snapshot()
}

macro_rules! mutation {
    ($command:ident, $arg:ident: $type:ty, $result:ty) => {
        #[tauri::command]
        async fn $command(
            state: tauri::State<'_, Controller>,
            $arg: $type,
        ) -> Result<$result, String> {
            let result = state.$command($arg).await;
            if !matches!(stringify!($command), "begin_probe" | "run_probe") {
                state.record_activity(stringify!($command), result.is_ok());
            }
            result
        }
    };
}

mutation!(save_jumper, jumper: Jumper, u64);
mutation!(import_command_tunnels, request: ssh_tunnel_core::repository::CommandImport, ssh_tunnel_core::repository::TunnelBatchImport);

#[tauri::command]
fn parse_ssh_command(
    command: String,
) -> Result<ssh_tunnel_core::ssh_command::CommandPreview, String> {
    ssh_tunnel_core::ssh_command::parse(&command)
}
mutation!(import_jumpers, jumpers: Vec<Jumper>, ssh_tunnel_core::repository::BatchImport);
mutation!(delete_jumper, id: u64, ());
mutation!(save_tunnel, tunnel: Tunnel, u64);
mutation!(delete_tunnel, id: u64, ());
mutation!(save_group, group: Group, u64);
mutation!(delete_group, id: u64, ());
mutation!(reorder_groups, ids: Vec<u64>, ());
mutation!(set_traffic_monitor, enabled: bool, ());
mutation!(begin_probe, request: ssh_tunnel_core::probe_session::Request, u64);
mutation!(run_probe, token: u64, ssh_tunnel_core::probe::Report);
mutation!(approve_probe_key, token: u64, ());

#[tauri::command]
async fn cancel_probe(state: tauri::State<'_, Controller>, token: u64) -> Result<(), String> {
    state.cancel_probe(token).await;
    Ok(())
}

#[tauri::command]
async fn start_tunnel(state: tauri::State<'_, Controller>, id: u64) -> Result<(), String> {
    state.start(id).await
}
#[tauri::command]
async fn stop_tunnel(state: tauri::State<'_, Controller>, id: u64) -> Result<(), String> {
    state.stop(id).await;
    Ok(())
}
#[tauri::command]
fn get_runtime(state: tauri::State<'_, Controller>) -> Vec<TunnelSnapshot> {
    state.runtime_state()
}
#[tauri::command]
fn get_tunnel_logs(state: tauri::State<'_, Controller>, id: u64) -> Vec<RuntimeEvent> {
    state.logs(id)
}

#[tauri::command]
fn get_activity_logs(state: tauri::State<'_, Controller>) -> Vec<RuntimeEvent> {
    state.activity_logs()
}
#[tauri::command]
fn get_pending_trust(state: tauri::State<'_, Controller>) -> Vec<RuntimeEvent> {
    state.pending_trust()
}
#[tauri::command]
async fn resolve_host_key(
    state: tauri::State<'_, Controller>,
    id: u64,
    generation: u64,
    accept: bool,
) -> Result<(), String> {
    state.resolve_host_key(id, generation, accept).await
}

fn main() {
    startup_trace::mark("main.enter");
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(not(windows))]
    let builder = builder.plugin(
        tauri_plugin_autostart::Builder::new()
            .app_name(login::entry_name())
            .args([
                "--config".to_string(),
                configuration_path()
                    .expect("Invalid config path")
                    .to_string_lossy()
                    .into_owned(),
            ])
            .build(),
    );
    startup_trace::mark("builder.begin");
    let app = builder
        .on_page_load(|_, _| startup_trace::mark("webview.page_load"))
        .setup(|app| {
            startup_trace::mark("setup.enter");
            if app.get_webview_window("main").is_some() {
                startup_trace::mark("main_window.present");
            } else {
                startup_trace::mark("main_window.absent");
            }
            let path = configuration_path().map_err(std::io::Error::other)?;
            let known_hosts = match std::env::var_os("SSH_TUNNEL_KNOWN_HOSTS") {
                Some(path) => {
                    let path = PathBuf::from(path);
                    if !path.is_absolute() {
                        return Err(std::io::Error::other(
                            "SSH_TUNNEL_KNOWN_HOSTS must be absolute",
                        )
                        .into());
                    }
                    path
                }
                None => PathBuf::from(
                    std::env::var_os("USERPROFILE")
                        .or_else(|| std::env::var_os("HOME"))
                        .ok_or_else(|| std::io::Error::other("Cannot locate home directory"))?,
                )
                .join(".ssh")
                .join("known_hosts"),
            };
            startup_trace::mark("controller.open.begin");
            let controller = Controller::open(path.clone(), known_hosts)?;
            startup_trace::mark("tray.setup.begin");
            desktop::setup(app, &controller.config_path())?;
            startup_trace::mark("tray.setup.done");
            app.manage(
                login::NativeLogin::new(app.handle().clone(), &path)
                    .map_err(std::io::Error::other)?,
            );
            let mut events = controller.subscribe();
            app.manage(controller);
            app.manage(StartupErrors::default());
            let startup_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut errors = startup_handle
                    .state::<Controller>()
                    .start_automatically()
                    .await;
                if let Err(error) = startup_handle
                    .state::<Controller>()
                    .reconcile_auto_run(&*startup_handle.state::<login::NativeLogin>())
                    .await
                {
                    errors.push(error);
                }
                *startup_handle
                    .state::<StartupErrors>()
                    .0
                    .lock()
                    .expect("Startup errors lock poisoned") = errors;
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("tunnel-event", event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = handle.emit("runtime-resync", ());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            startup_trace::mark("setup.done");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Only intercept when a tray exists, so a tray failure cannot strand the app.
                if window.app_handle().tray_by_id("main-tray").is_some() && window.hide().is_ok() {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            config_files::default_ssh_config_path,
            config_files::load_ssh_config,
            login::get_auto_run,
            login::set_auto_run,
            config_files::get_config_path,
            config_files::get_config_location,
            config_files::config_directory_conflicts,
            config_files::relocate_config,
            config_files::choose_config_directory,
            config_files::reveal_config_directory,
            config_files::choose_config_file,
            config_files::preview_import,
            config_files::confirm_import,
            config_files::cancel_import,
            config_files::export_config,
            desktop::get_ui_locale,
            desktop::set_ui_locale,
            take_startup_errors,
            quit_app,
            get_state,
            save_jumper,
            parse_ssh_command,
            import_command_tunnels,
            import_jumpers,
            delete_jumper,
            save_tunnel,
            delete_tunnel,
            save_group,
            delete_group,
            reorder_groups,
            set_traffic_monitor,
            begin_probe,
            run_probe,
            approve_probe_key,
            cancel_probe,
            start_tunnel,
            stop_tunnel,
            get_runtime,
            get_tunnel_logs,
            get_activity_logs,
            get_pending_trust,
            resolve_host_key
        ])
        .build(tauri::generate_context!())
        .inspect_err(|_| startup_trace::mark("builder.failed"))
        .expect("Failed to start ssh-tunnel");
    startup_trace::mark("builder.done");
    let exiting = Arc::new(AtomicU8::new(0));
    startup_trace::mark("event_loop.begin");
    app.run(move |handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && exiting.load(Ordering::SeqCst) != 2
        {
            api.prevent_exit();
            if exiting
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            let handle = handle.clone();
            let exiting = exiting.clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<Controller>().stop_all().await;
                exiting.store(2, Ordering::SeqCst);
                handle.exit(0);
            });
        }
    });
}
