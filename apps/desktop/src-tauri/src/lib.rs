//! Context desktop app: app setup, windows, the menu-bar tray and commands.

mod bridge;
mod context;
mod engines;
mod orchestrator;
mod platform;
mod pointing;
mod popover;
mod replay;
mod secrets;
mod settings;
mod tools;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};
use tauri_plugin_opener::OpenerExt;

use bridge::Bridge;

use engines::anthropic_api::{self, AnthropicApi};
use engines::{Effort, Engine, EngineInfo, Readiness};
use orchestrator::Orchestrator;
use orchestrator::summary::ProjectSummaries;
use platform::{
    InputHooks, Native, Overlay, OverlayKind, Permission, PermissionStatus, Permissions,
};
use pointing::Pointing;
use popover::Popover;
use replay::{PackSummary, PackView};
use secrets::{Keychain, Secrets};
use settings::SettingsStore;
use tools::ProjectToolProvider;
use tools::sandbox::Sandbox;

const SETTINGS_WINDOW: &str = "settings";
const VIEWER_WINDOW: &str = "viewer";
/// The keychain service Context's secrets are filed under.
const KEYCHAIN_SERVICE: &str = "dev.context.app";

const MENU_ENABLED: &str = "enabled";
const MENU_SETTINGS: &str = "settings";
const MENU_VIEWER: &str = "viewer";
const MENU_QUIT: &str = "quit";

/// Where dev mode saves context packs. `None` outside dev mode.
struct Packs(Option<PathBuf>);

/// The AI engine in use. (M5 adds a second one to choose from.)
type ActiveEngine = Arc<dyn Engine>;

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            permission_status,
            request_permission,
            list_packs,
            load_pack,
            open_viewer,
            open_settings,
            engine_status,
            save_api_key,
            remove_api_key,
            effort_ceiling,
            set_effort_ceiling,
            popover_ask,
            popover_go_deeper,
            popover_set_effort,
            popover_correct,
            popover_close,
            popover_sent,
            open_file
        ])
        .setup(|app| {
            // Menu bar only: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let native = Arc::new(Native);
            for (label, kind) in [
                (pointing::LENS_WINDOW, OverlayKind::Lens),
                (popover::POPOVER_WINDOW, OverlayKind::Popover),
            ] {
                let window = app
                    .get_webview_window(label)
                    .expect("overlay windows are declared in tauri.conf.json");
                if let Err(e) = native.configure_overlay(&window, kind) {
                    log::warn!("{label} overlay unavailable: {e}");
                }
            }

            let settings = Arc::new(SettingsStore::load(
                app.path().app_config_dir()?.join("settings.json"),
            ));
            let dev_mode = settings.get().dev_mode;
            let data_dir = app.path().app_data_dir()?;
            let packs_dir = dev_mode.then(|| data_dir.join("packs"));
            let bridge = match tauri::async_runtime::block_on(Bridge::start(&data_dir)) {
                Ok(bridge) => Some(bridge),
                Err(e) => {
                    log::warn!("the VS Code bridge didn't start: {e}");
                    None
                }
            };

            let secrets = Arc::new(Secrets::new(Box::new(Keychain::new(KEYCHAIN_SERVICE))));
            let engine: ActiveEngine =
                Arc::new(AnthropicApi::new(engine_config()?, secrets.clone()));
            let popover = Arc::new(Popover::new(app.handle().clone(), native.clone()));
            let orchestrator = Arc::new(Orchestrator::new(
                engine.clone(),
                settings.clone(),
                popover.clone(),
                Arc::new(ProjectToolProvider::new()),
                Some(Arc::new(ProjectSummaries::new(data_dir.join("projects")))),
            ));

            let pointing = Pointing::start(pointing::Parts {
                app: app.handle().clone(),
                native: native.clone(),
                settings: settings.clone(),
                popover: popover.clone(),
                orchestrator: orchestrator.clone(),
                bridge,
                packs_dir: packs_dir.clone(),
            });
            if let Err(e) = native.start_input(pointing.clone()) {
                log::warn!("input hooks unavailable: {e}");
            }
            build_tray(app.handle(), pointing.clone(), dev_mode)?;

            // First run, or a permission was taken away: show onboarding. Dev
            // builds always open Settings, as the menu-bar icon can hide
            // behind the notch.
            if dev_mode || !native.permission_status().all_granted() {
                show_settings(app.handle());
            }

            app.manage(native);
            app.manage(pointing);
            app.manage(Packs(packs_dir));
            app.manage(settings);
            app.manage(secrets);
            app.manage(engine);
            app.manage(popover);
            app.manage(orchestrator);
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

/// The Claude API engine's section of config/models.json.
fn engine_config() -> Result<anthropic_api::Config, serde_json::Error> {
    let models: serde_json::Value =
        serde_json::from_str(include_str!("../../../../config/models.json"))?;
    serde_json::from_value(models[anthropic_api::ENGINE_ID].clone())
}

#[tauri::command]
fn permission_status(native: State<'_, Arc<Native>>) -> PermissionStatus {
    native.permission_status()
}

#[tauri::command]
fn request_permission(native: State<'_, Arc<Native>>, which: Permission) {
    native.request_permission(which);
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
    Ok(replay::view(&pack))
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    info: EngineInfo,
    readiness: Readiness,
}

#[tauri::command]
async fn engine_status(engine: State<'_, ActiveEngine>) -> Result<EngineStatus, ()> {
    Ok(EngineStatus {
        info: engine.info(),
        readiness: engine.check_ready().await,
    })
}

/// The key goes one way: from Settings into the keychain. Nothing sends it
/// back to any window.
#[tauri::command]
fn save_api_key(
    key: String,
    secrets: State<'_, Arc<Secrets>>,
    engine: State<'_, ActiveEngine>,
) -> Result<(), String> {
    secrets
        .set(engine.info().id, &key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_api_key(
    secrets: State<'_, Arc<Secrets>>,
    engine: State<'_, ActiveEngine>,
) -> Result<(), String> {
    secrets.delete(engine.info().id).map_err(|e| e.to_string())
}

#[tauri::command]
fn effort_ceiling(settings: State<'_, Arc<SettingsStore>>) -> Effort {
    settings.get().effort_ceiling
}

#[tauri::command]
fn set_effort_ceiling(
    effort: Effort,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    settings
        .update(|s| s.effort_ceiling = effort)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn popover_ask(question: String, orchestrator: State<'_, Arc<Orchestrator>>) {
    orchestrator.ask(question);
}

#[tauri::command]
fn popover_go_deeper(orchestrator: State<'_, Arc<Orchestrator>>) {
    orchestrator.go_deeper();
}

#[tauri::command]
fn popover_set_effort(effort: Effort, orchestrator: State<'_, Arc<Orchestrator>>) {
    orchestrator.set_effort(effort);
}

#[tauri::command]
fn popover_correct(target: String, orchestrator: State<'_, Arc<Orchestrator>>) {
    orchestrator.correct_target(target);
}

#[tauri::command]
fn popover_close(popover: State<'_, Arc<Popover>>, orchestrator: State<'_, Arc<Orchestrator>>) {
    popover.close();
    orchestrator.cancel();
}

/// "What was sent": the context pack, and what the tools read since.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sent {
    view: PackView,
    activity: Vec<String>,
}

#[tauri::command]
fn popover_sent(orchestrator: State<'_, Arc<Orchestrator>>) -> Option<Sent> {
    orchestrator.sent().map(|(pack, activity)| Sent {
        view: replay::view(&pack),
        activity,
    })
}

/// Opens a project file from an answer's link in the editor, at a line.
/// The path comes from the model, so it goes through the sandbox first.
#[tauri::command]
fn open_file(
    path: String,
    line: Option<u32>,
    app: AppHandle,
    orchestrator: State<'_, Arc<Orchestrator>>,
) -> Result<(), String> {
    let pack = orchestrator.pack().ok_or("there's no answer open")?;
    let workspace = pack.workspace.as_ref().ok_or("not in a code project")?;
    let file = Sandbox::new(&workspace.roots)
        .resolve(&path)
        .map_err(|e| e.to_string())?;
    app.opener()
        .open_url(editor_url(&workspace.uri_scheme, &file, line), None::<&str>)
        .map_err(|e| e.to_string())
}

/// `vscode://file/<absolute path>:<line>`, in the editor's own scheme.
fn editor_url(scheme: &str, file: &Path, line: Option<u32>) -> String {
    let scheme = if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    {
        scheme
    } else {
        "vscode"
    };
    let mut path = String::new();
    for byte in file.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            path.push(byte as char);
        } else {
            path.push_str(&format!("%{byte:02X}"));
        }
    }
    let line = line.map(|l| format!(":{l}")).unwrap_or_default();
    format!("{scheme}://file{path}{line}")
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
    fn editor_urls_are_encoded_and_scheme_checked() {
        use std::path::Path;
        assert_eq!(
            super::editor_url("vscode", Path::new("/work/my shop/src/server.ts"), Some(12)),
            "vscode://file/work/my%20shop/src/server.ts:12"
        );
        assert_eq!(
            super::editor_url("cursor", Path::new("/a.ts"), None),
            "cursor://file/a.ts"
        );
        assert_eq!(
            super::editor_url("javascript:alert(1)//", Path::new("/a.ts"), None),
            "vscode://file/a.ts"
        );
    }

    #[test]
    fn sources_config_is_valid_json() {
        assert!(read_config("sources.json").is_object());
    }
}
