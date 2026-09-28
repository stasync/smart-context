//! Context desktop app: app setup, windows, the menu-bar tray and commands.

mod bridge;
mod context;
mod engines;
mod orchestrator;
mod platform;
mod pointing;
mod replay;
mod secrets;
mod settings;
mod tools;

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WindowEvent};

use platform::{InputHooks, Native, Overlay, Permission, PermissionStatus, Permissions};
use pointing::Pointing;
use settings::Settings;

const SETTINGS_WINDOW: &str = "settings";

const MENU_ENABLED: &str = "enabled";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            permission_status,
            request_permission
        ])
        .setup(|app| {
            // Menu bar only: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let native = Arc::new(Native);
            let lens = app
                .get_webview_window(pointing::LENS_WINDOW)
                .expect("the lens window is declared in tauri.conf.json");
            if let Err(e) = native.configure_overlay(&lens) {
                log::warn!("lens overlay unavailable: {e}");
            }

            let pointing =
                Pointing::start(app.handle().clone(), native.clone(), Settings::default());
            if let Err(e) = native.start_input(pointing.clone()) {
                log::warn!("input hooks unavailable: {e}");
            }
            build_tray(app.handle(), pointing.clone())?;

            // First run, or a permission was taken away: show onboarding.
            if !native.permission_status().all_granted() {
                show_settings(app.handle());
            }

            app.manage(native);
            app.manage(pointing);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing Settings only hides it; the app keeps running until Quit.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == SETTINGS_WINDOW
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the Context app");
}

#[tauri::command]
fn permission_status(native: State<'_, Arc<Native>>) -> PermissionStatus {
    native.permission_status()
}

#[tauri::command]
fn request_permission(native: State<'_, Arc<Native>>, which: Permission) {
    native.request_permission(which);
}

fn build_tray(app: &AppHandle, pointing: Arc<Pointing>) -> tauri::Result<()> {
    let enabled = CheckMenuItem::with_id(app, MENU_ENABLED, "Enabled", true, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Context", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &enabled,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true)
        .tooltip("Context")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            // The menu toggles the checkmark itself; follow it.
            MENU_ENABLED => pointing.set_enabled(enabled.is_checked().unwrap_or(true)),
            MENU_SETTINGS => show_settings(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    fn read_config(name: &str) -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../config/");
        let text = std::fs::read_to_string(format!("{path}{name}"))
            .unwrap_or_else(|e| panic!("reading config/{name}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing config/{name}: {e}"))
    }

    #[test]
    fn models_config_has_every_effort_level_for_every_engine() {
        let models = read_config("models.json");
        for engine in ["anthropic_api", "claude_code"] {
            for effort in ["low", "medium", "high", "max"] {
                assert!(
                    models[engine][effort]["model"].is_string(),
                    "config/models.json is missing {engine}.{effort}.model"
                );
            }
        }
    }

    #[test]
    fn sources_config_is_valid_json() {
        assert!(read_config("sources.json").is_object());
    }
}
