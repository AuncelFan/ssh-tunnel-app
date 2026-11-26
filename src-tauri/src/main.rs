#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_files;
mod desktop;
mod startup_trace;

use serde_json::{Value, json};
use ssh_tunnel::{Client, cli, configuration_path};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tauri::{Emitter, Manager};

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn desktop_commands() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        config_files::default_ssh_config_path,
        config_files::choose_config_directory,
        config_files::reveal_config_directory,
        config_files::choose_config_file,
        desktop::get_ui_locale,
        desktop::set_ui_locale,
        quit_app
    ]
}

fn main() {
    let positional = cli::arguments();
    match positional.first().map(String::as_str) {
        Some("--backend") => {
            let result = configuration_path()
                .map_err(Into::into)
                .and_then(|path| ssh_tunnel::serve(&path));
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
            return;
        }
        Some("cli" | "--help" | "-h" | "--version" | "-V") => {
            #[cfg(all(windows, not(debug_assertions)))]
            {
                unsafe {
                    windows_sys::Win32::System::Console::AttachConsole(
                        windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
                    );
                }
            }
            let commands = if positional[0] == "cli" {
                &positional[1..]
            } else {
                &positional[..]
            };
            std::process::exit(cli::execute(commands, cli::Entry::Desktop));
        }
        _ => {}
    }
    startup_trace::mark("main.enter");
    let exiting = Arc::new(AtomicU8::new(0));
    let backend_exit = exiting.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|_, _| startup_trace::mark("webview.page_load"))
        .setup(move |app| {
            startup_trace::mark("setup.enter");
            let path = configuration_path().map_err(std::io::Error::other)?;
            let client = Client::new(&path)?;
            client.ensure(&path, &std::env::current_exe()?)?;
            desktop::setup(app, &client.config_path().map_err(std::io::Error::other)?)?;
            app.manage(client.clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let _ = client.events(|event| {
                    if event.is_null() {
                        let _ = handle.emit("runtime-resync", ());
                    } else {
                        let _ = handle.emit("tunnel-event", event);
                    }
                });
                backend_exit.store(2, Ordering::SeqCst);
                handle.exit(0);
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
        .invoke_handler(move |invoke| {
            let command = invoke.message.command().to_owned();
            if matches!(
                command.as_str(),
                "default_ssh_config_path"
                    | "choose_config_directory"
                    | "reveal_config_directory"
                    | "choose_config_file"
                    | "get_ui_locale"
                    | "set_ui_locale"
                    | "quit_app"
            ) {
                return desktop_commands()(invoke);
            }
            if matches!(command.as_str(), "shutdown" | "subscribe") {
                return false;
            }
            let tauri::ipc::InvokeBody::Json(args) = invoke.message.payload() else {
                return false;
            };
            let args = args.clone();
            let handle = invoke.message.webview_ref().app_handle().clone();
            invoke.resolver.respond_async(async move {
                tauri::async_runtime::spawn_blocking(move || -> Result<Value, String> {
                    let client = handle.state::<Client>();
                    let mut result = client.call(&command, args)?;
                    if command == "relocate_config"
                        && let Err(error) = desktop::reload_locale(
                            &handle.state::<desktop::Desktop>(),
                            &client.config_path()?,
                        )
                        && let Some(warnings) = result.as_array_mut()
                    {
                        warnings.push(Value::String(error));
                    }
                    Ok(result)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(Into::into)
            });
            true
        })
        .build(tauri::generate_context!())
        .inspect_err(|_| startup_trace::mark("builder.failed"))
        .expect("Failed to start ssh-tunnel");
    startup_trace::mark("builder.done");
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
                let client = handle.state::<Client>().inner().clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    client.call("shutdown", json!({}))
                })
                .await;
                exiting.store(2, Ordering::SeqCst);
                handle.exit(0);
            });
        }
    });
}
