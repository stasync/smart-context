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

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};

use context::ContextPack;
use platform::{InputHooks, Native, Overlay, Permission, PermissionStatus, Permissions};
use pointing::Pointing;
use replay::PackSummary;
use settings::Settings;

const SETTINGS_WINDOW: &str = "settings";
const VIEWER_WINDOW: &str = "viewer";

const MENU_ENABLED: &str = "enabled";
const MENU_SETTINGS: &str = "settings";
const MENU_VIEWER: &str = "viewer";
const MENU_QUIT: &str = "quit";

/// Where dev mode saves context packs. `None` outside dev mode.
struct Packs(Option<PathBuf>);

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            permission_status,
            request_permission,
            list_packs,
            load_pack,
            open_viewer
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

            let settings = Settings::default();
            let packs_dir = settings
                .dev_mode
                .then(|| app.path().app_data_dir().map(|d| d.join("packs")))
                .transpose()?;
            let pointing = Pointing::start(
                app.handle().clone(),
                native.clone(),
                settings.clone(),
                packs_dir.clone(),
            );
            if let Err(e) = native.start_input(pointing.clone()) {
                log::warn!("input hooks unavailable: {e}");
            }
            build_tray(app.handle(), pointing.clone(), settings.dev_mode)?;

            // First run, or a permission was taken away: show onboarding. Dev
            // builds always open Settings, as the menu-bar icon can hide
            // behind the notch.
            if settings.dev_mode || !native.permission_status().all_granted() {
                show_settings(app.handle());
            }

            app.manage(native);
            app.manage(pointing);
            app.manage(Packs(packs_dir));
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
        .build(tauri::generate_context!())
        .expect("error while building the Context app")
        .run(|app, event| {
            if platform::is_reopen(&event) {
                show_settings(app);
            }
        });
}

#[tauri::command]
fn permission_status(native: State<'_, Arc<Native>>) -> PermissionStatus {
    native.permission_status()
}

#[tauri::command]
fn request_permission(native: State<'_, Arc<Native>>, which: Permission) {
    native.request_permission(which);
}

/// A saved pack with its images inline, for the dev viewer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackView {
    pack: ContextPack,
    lens_image_url: Option<String>,
    window_image_url: Option<String>,
}

#[tauri::command]
fn list_packs(packs: State<'_, Packs>) -> Result<Vec<PackSummary>, String> {
    match &packs.0 {
        Some(dir) => replay::list(dir).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
fn load_pack(packs: State<'_, Packs>, key: String) -> Result<PackView, String> {
    let dir = packs.0.as_ref().ok_or("dev mode is off")?;
    let pack = replay::load(dir, &key).map_err(|e| e.to_string())?;
    Ok(PackView {
        lens_image_url: pack.lens_image.as_ref().map(replay::data_url),
        window_image_url: pack.window_image.as_ref().map(replay::data_url),
        pack,
    })
}

/// Async, so the window isn't built while the main thread waits on the command.
#[tauri::command]
async fn open_viewer(app: AppHandle) {
    show_viewer(&app);
}

fn build_tray(app: &AppHandle, pointing: Arc<Pointing>, dev_mode: bool) -> tauri::Result<()> {
    let enabled = CheckMenuItem::with_id(app, MENU_ENABLED, "Enabled", true, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let viewer = MenuItem::with_id(app, MENU_VIEWER, "Captures…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Context", true, Some("CmdOrCtrl+Q"))?;
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&enabled, &settings];
    if dev_mode {
        items.push(&viewer);
    }
    items.extend([&separator as &dyn IsMenuItem<Wry>, &quit]);
    let menu = Menu::with_items(app, &items)?;

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
            MENU_VIEWER => show_viewer(app),
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

/// The dev-only viewer for saved context packs.
fn show_viewer(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(VIEWER_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built =
        WebviewWindowBuilder::new(app, VIEWER_WINDOW, WebviewUrl::App("viewer.html".into()))
            .title("Context Captures")
            .inner_size(1200.0, 860.0)
            .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => log::warn!("couldn't open the capture viewer: {e}"),
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
