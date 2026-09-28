//! Context desktop app: app setup, windows and the menu-bar tray.

mod bridge;
mod context;
mod engines;
mod orchestrator;
mod platform;
mod replay;
mod secrets;
mod settings;
mod tools;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

const SETTINGS_WINDOW: &str = "settings";

const MENU_ENABLED: &str = "enabled";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

/// App-wide state shared between the tray and (from M1) the input thread.
struct AppState {
    /// Whether pointing is on. Toggled from the tray menu.
    enabled: AtomicBool,
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            enabled: AtomicBool::new(true),
        })
        .setup(|app| {
            // Menu bar only: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            build_tray(app.handle())?;

            // A menu-bar icon can hide behind the notch, so dev builds open
            // Settings on launch. M1 replaces this with first-run onboarding.
            if cfg!(debug_assertions) {
                show_settings(app.handle());
            }
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

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
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
            MENU_ENABLED => {
                // The menu toggles the checkmark itself; mirror it into state.
                let on = enabled.is_checked().unwrap_or(true);
                app.state::<AppState>().enabled.store(on, Ordering::Relaxed);
            }
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
