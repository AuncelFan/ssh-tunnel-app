use std::sync::Mutex;
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub struct Desktop {
    locale: Mutex<String>,
    show: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

fn normalize(raw: &str) -> &'static str {
    if raw.trim().to_ascii_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en"
    }
}

fn labels(locale: &str) -> (&'static str, &'static str) {
    if locale == "zh-CN" {
        (
            "\u{663e}\u{793a}\u{4e3b}\u{7a97}\u{53e3}",
            "\u{9000}\u{51fa}",
        )
    } else {
        ("Show window", "Quit")
    }
}

pub fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn setup(app: &tauri::App, config: &std::path::Path) -> tauri::Result<()> {
    let locale_path = config.with_file_name("ui.locale");
    let saved = std::fs::read_to_string(&locale_path).ok();
    let environment = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty());
    let locale = normalize(saved.as_deref().or(environment.as_deref()).unwrap_or("en"));
    let (show_label, quit_label) = labels(locale);
    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("ssh-tunnel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(Desktop {
        locale: Mutex::new(locale.into()),
        show,
        quit,
    });
    Ok(())
}

#[tauri::command]
pub fn get_ui_locale(state: tauri::State<'_, Desktop>) -> String {
    state.locale.lock().expect("Locale lock poisoned").clone()
}

#[tauri::command]
pub fn set_ui_locale(
    state: tauri::State<'_, Desktop>,
    controller: tauri::State<'_, ssh_tunnel_core::controller::Controller>,
    locale: String,
) -> Result<(), String> {
    if !["en", "zh-CN"].contains(&locale.as_str()) {
        return Err("Unsupported UI language".into());
    }
    let mut current = state.locale.lock().expect("Locale lock poisoned");
    let locale_path = controller.config_path().with_file_name("ui.locale");
    let parent = locale_path
        .parent()
        .ok_or("Missing configuration directory")?;
    std::fs::create_dir_all(parent).map_err(|_| "Cannot create locale directory")?;
    std::fs::write(&locale_path, format!("{locale}\n")).map_err(|_| "Cannot save UI language")?;
    let (show, quit) = labels(&locale);
    state
        .show
        .set_text(show)
        .map_err(|_| "Cannot update tray label")?;
    state
        .quit
        .set_text(quit)
        .map_err(|_| "Cannot update tray label")?;
    *current = locale;
    Ok(())
}

pub fn reload_locale(state: &Desktop, config: &std::path::Path) -> Result<(), String> {
    let mut current = state.locale.lock().expect("Locale lock poisoned");
    let saved = match std::fs::read_to_string(config.with_file_name("ui.locale")) {
        Ok(saved) => saved,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Cannot read relocated UI language".into()),
    };
    let locale = normalize(&saved);
    let (show, quit) = labels(locale);
    state
        .show
        .set_text(show)
        .map_err(|_| "Cannot update tray label")?;
    state
        .quit
        .set_text(quit)
        .map_err(|_| "Cannot update tray label")?;
    *current = locale.into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_normalization_matches_legacy_supported_languages() {
        for raw in ["zh-CN", " zh_TW ", "ZH-hant"] {
            assert_eq!(normalize(raw), "zh-CN");
        }
        for raw in ["", "en-US", "de"] {
            assert_eq!(normalize(raw), "en");
        }
        assert_eq!(labels("en"), ("Show window", "Quit"));
        assert_ne!(labels("en"), labels("zh-CN"));
    }
}
