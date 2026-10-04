//! scrin desktop: a Tauri 2 shell around `scrin-engine`.
//!
//! The webview renders the shared React UI (apps/web built with
//! `VITE_SCRIN_HOST=desktop`) and talks to the engine only through the
//! `scrin_*` commands below. Video is drawn natively ([`video`]). Closing the
//! window hides it to the tray so the device stays reachable; Quit from the
//! tray ends sessions and exits.

pub mod bridge;
pub mod video;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use scrin_engine::{Command, EngineConfig, Reply};
use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tracing::{info, warn};

use bridge::{Bridge, Connected, UiInput, parse_quality, special_key_events};
use video::{Presenter, Rect};

/// Event carrying a `scrin://` deep link to the webview.
const DEEP_LINK_EVENT: &str = "scrin://deep-link";
/// Desktop settings the engine needs before it starts (`<app data>/settings.json`).
const SETTINGS_FILE: &str = "settings.json";

struct AppState {
    bridge: Arc<Bridge>,
    presenter: Arc<Presenter>,
    quitting: AtomicBool,
}

type Res<T> = Result<T, String>;

fn ok(r: Reply) -> Res<serde_json::Value> {
    Ok(match r {
        Reply::Status(s) => serde_json::to_value(s).map_err(|e| e.to_string())?,
        Reply::Session(s) => {
            serde_json::to_value(Connected { session_id: s }).map_err(|e| e.to_string())?
        }
        Reply::Trusted(t) => serde_json::to_value(t).map_err(|e| e.to_string())?,
        // `Ok` and any future reply without a payload.
        _ => serde_json::Value::Null,
    })
}

// ---- commands: UI → engine ----------------------------------------------

#[tauri::command]
async fn scrin_status(st: State<'_, AppState>) -> Res<serde_json::Value> {
    ok(st.bridge.call_async(Command::GetStatus).await?)
}

#[tauri::command]
async fn scrin_regenerate_code(st: State<'_, AppState>) -> Res<serde_json::Value> {
    ok(st.bridge.call_async(Command::RegenerateCode).await?)
}

/// D24: show a five-word passphrase in the UI language (`lang` = `en`/`ro`).
#[tauri::command]
async fn scrin_enable_phrase(
    st: State<'_, AppState>,
    lang: Option<String>,
) -> Res<serde_json::Value> {
    let lang = lang.unwrap_or_default();
    if lang.len() > 16 {
        return Err("input too long".into());
    }
    ok(st.bridge.call_async(Command::EnablePhrase { lang }).await?)
}

#[tauri::command]
async fn scrin_disable_phrase(st: State<'_, AppState>) -> Res<serde_json::Value> {
    ok(st.bridge.call_async(Command::DisablePhrase).await?)
}

#[tauri::command]
async fn scrin_connect(
    st: State<'_, AppState>,
    target: String,
    code: String,
    requested: Option<Vec<String>>,
) -> Res<serde_json::Value> {
    if target.len() > 4096 || code.len() > 64 {
        return Err("input too long".into());
    }
    ok(st
        .bridge
        .call_async(Command::Connect {
            target,
            code,
            requested,
        })
        .await?)
}

#[tauri::command]
async fn scrin_confirm_sas(st: State<'_, AppState>, session: String, matches: bool) -> Res<()> {
    st.bridge
        .call_async(Command::ConfirmSas { session, matches })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_accept(
    st: State<'_, AppState>,
    session: String,
    permissions: Vec<String>,
) -> Res<()> {
    st.bridge
        .call_async(Command::Accept {
            session,
            permissions,
        })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_reject(st: State<'_, AppState>, session: String) -> Res<()> {
    st.bridge
        .call_async(Command::Reject { session })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_set_permission(
    st: State<'_, AppState>,
    session: String,
    permission: String,
    granted: bool,
) -> Res<()> {
    let cmd = if granted {
        Command::Grant {
            session,
            permission,
        }
    } else {
        Command::Revoke {
            session,
            permission,
        }
    };
    st.bridge.call_async(cmd).await.map(drop)
}

#[tauri::command]
async fn scrin_end_session(st: State<'_, AppState>, session: String) -> Res<()> {
    st.presenter.clear();
    st.bridge
        .call_async(Command::EndSession { session })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_send_input(st: State<'_, AppState>, session: String, input: UiInput) -> Res<()> {
    st.bridge
        .call_async(Command::SendInput {
            session,
            event: input.into(),
        })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_send_keys(st: State<'_, AppState>, session: String, combo: String) -> Res<()> {
    let events = special_key_events(&combo).ok_or("unknown key combo")?;
    for event in events {
        st.bridge
            .call_async(Command::SendInput {
                session: session.clone(),
                event,
            })
            .await?;
    }
    Ok(())
}

#[tauri::command]
async fn scrin_set_quality(st: State<'_, AppState>, session: String, quality: String) -> Res<()> {
    let quality = parse_quality(&quality).ok_or("unknown quality")?;
    st.bridge
        .call_async(Command::SetQuality { session, quality })
        .await
        .map(drop)
}

#[tauri::command]
async fn scrin_list_trusted(st: State<'_, AppState>) -> Res<serde_json::Value> {
    ok(st.bridge.call_async(Command::ListTrusted).await?)
}

#[tauri::command]
async fn scrin_remove_trusted(st: State<'_, AppState>, device: String) -> Res<serde_json::Value> {
    ok(st
        .bridge
        .call_async(Command::RemoveTrusted { device })
        .await?)
}

#[tauri::command]
async fn scrin_trust_peer(st: State<'_, AppState>, session: String, label: String) -> Res<()> {
    st.bridge
        .call_async(Command::TrustPeer { session, label })
        .await
        .map(drop)
}

/// Canvas rectangle in CSS pixels; `null` hides the native video surface.
#[derive(Debug, Deserialize)]
struct CssRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

// Tauri's command macro hands extractors over by value; the Result is the
// IPC contract (the JS side awaits a resolved promise).
#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
#[tauri::command]
fn scrin_video_rect(app: AppHandle, st: State<'_, AppState>, rect: Option<CssRect>) -> Res<()> {
    let scale = app
        .get_webview_window("main")
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    #[expect(clippy::cast_possible_truncation)] // window coordinates fit i32
    let px = |v: f64| (v * scale).round() as i32;
    st.presenter.set_rect(rect.map(|r| Rect {
        x: px(r.x),
        y: px(r.y),
        width: px(r.width),
        height: px(r.height),
    }));
    Ok(())
}

#[expect(clippy::needless_pass_by_value)] // Tauri's command macro passes AppHandle by value
#[tauri::command]
fn scrin_quit(app: AppHandle) {
    quit(&app);
}

/// Settings → Network → Server. `SCRIN_SERVER` (env) wins over the file.
#[derive(Debug, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopSettings {
    #[serde(default)]
    server: Option<String>,
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join(SETTINGS_FILE))
}

fn load_settings(app: &AppHandle) -> DesktopSettings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// `http(s)://host[:port]` with nothing after the authority but an optional `/`.
fn valid_server(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else {
        return false;
    };
    let rest = rest.trim_end_matches('/');
    matches!(scheme, "http" | "https")
        && !rest.is_empty()
        && rest.len() <= 253
        && !rest.contains(['/', '?', '#', '@', ' '])
}

/// The effective server (env, else settings).
fn effective_server(app: &AppHandle) -> Option<String> {
    std::env::var("SCRIN_SERVER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| load_settings(app).server)
        .map(|s| s.trim().trim_end_matches('/').to_owned())
        .filter(|s| valid_server(s))
}

#[expect(clippy::needless_pass_by_value)] // Tauri's command macro passes AppHandle by value
#[tauri::command]
fn scrin_get_server(app: AppHandle) -> Option<String> {
    effective_server(&app)
}

/// Saves the server; applies on the next start (the engine binds its relays
/// at start). An empty string clears it (serverless: tickets and LAN only).
#[expect(clippy::needless_pass_by_value)] // Tauri's command macro passes AppHandle by value
#[tauri::command]
fn scrin_set_server(app: AppHandle, server: String) -> Res<()> {
    let server = server.trim().trim_end_matches('/').to_owned();
    if !server.is_empty() && !valid_server(&server) {
        return Err("server must be http(s)://host[:port]".into());
    }
    let path = settings_path(&app).ok_or("no app data directory")?;
    let mut s = load_settings(&app);
    s.server = (!server.is_empty()).then_some(server);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(&s).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

// ---- window, tray, lifecycle ---------------------------------------------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The one exit path: end sessions, stop the engine, then exit.
fn quit(app: &AppHandle) {
    if let Some(st) = app.try_state::<AppState>() {
        if st.quitting.swap(true, Ordering::AcqRel) {
            return;
        }
        st.presenter.stop();
        st.bridge.shutdown();
    }
    info!("quit");
    app.exit(0);
}

fn tray_copy_id(app: &AppHandle) {
    let Some(st) = app.try_state::<AppState>() else {
        return;
    };
    if let Ok(Reply::Status(s)) = st.bridge.call(Command::GetStatus) {
        let _ = app.clipboard().write_text(s.scrin_id);
    }
}

fn tray_new_code(app: &AppHandle) {
    let Some(st) = app.try_state::<AppState>() else {
        return;
    };
    if let Err(e) = st.bridge.call(Command::RegenerateCode) {
        warn!(error = %e, "could not regenerate the code");
    }
}

/// Tray labels follow the OS locale (EN default, RO when the OS is Romanian);
/// the webview owns all other strings through i18next.
fn tray_labels() -> [&'static str; 4] {
    let ro = tauri_plugin_os::locale().is_some_and(|l| l.to_ascii_lowercase().starts_with("ro"));
    if ro {
        ["Afișează scrin", "Copiază ID-ul", "Cod nou", "Ieșire"]
    } else {
        ["Show scrin", "Copy ID", "New code", "Quit"]
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let [show, copy, code, quit_label] = tray_labels();
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", show, true, None::<&str>)?,
            &MenuItem::with_id(app, "copy-id", copy, true, None::<&str>)?,
            &MenuItem::with_id(app, "new-code", code, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("scrin")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "show" => show_main(app),
            "copy-id" => tray_copy_id(app),
            "new-code" => tray_new_code(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// `scrin://connect/<id>` (and argv of a second instance) → the webview.
fn forward_deep_links(app: &AppHandle, args: &[String]) {
    for a in args.iter().filter(|a| a.starts_with("scrin://")) {
        if a.len() <= 2048 {
            let _ = app.emit(DEEP_LINK_EVENT, a.clone());
        }
    }
}

fn engine_config(app: &AppHandle) -> Result<EngineConfig, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut cfg = EngineConfig::new(dir);
    // Registration, presence, signed resolve and the server's relays.
    cfg.server = effective_server(app);
    info!(
        server = cfg.server.as_deref().unwrap_or("-"),
        "engine config"
    );
    Ok(cfg)
}

#[cfg(windows)]
fn parent_hwnd(app: &AppHandle) -> isize {
    app.get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map_or(0, |h| h.0 as isize)
}

#[cfg(not(windows))]
fn parent_hwnd(_app: &AppHandle) -> isize {
    0
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let presenter = Arc::new(Presenter::new(parent_hwnd(&handle)));
    let bridge = Arc::new(Bridge::start(
        handle.clone(),
        engine_config(&handle)?,
        presenter.clone(),
    )?);
    {
        let b = bridge.clone();
        presenter.set_input_sink(Arc::new(move |session, input| {
            // Input from the native surface; fire and forget, never block the
            // window thread on the engine.
            let b = b.clone();
            std::thread::spawn(move || {
                let _ = b.call(Command::SendInput {
                    session,
                    event: input.into(),
                });
            });
        }));
    }
    app.manage(AppState {
        bridge,
        presenter,
        quitting: AtomicBool::new(false),
    });
    build_tray(&handle)?;
    #[cfg(any(windows, target_os = "linux"))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        if let Err(e) = app.deep_link().register_all() {
            warn!(error = %e, "could not register the scrin:// scheme");
        }
    }
    forward_deep_links(&handle, &std::env::args().collect::<Vec<_>>());
    Ok(())
}

/// Builds and runs the app. Returns when the app exits.
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
        )
        .try_init();

    let app = tauri::Builder::default()
        // Must be first: a second launch hands its argv (deep links) to us.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_main(app);
            forward_deep_links(app, &args);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--hidden"])
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                // Close to tray: the device stays reachable.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            scrin_status,
            scrin_regenerate_code,
            scrin_enable_phrase,
            scrin_disable_phrase,
            scrin_connect,
            scrin_confirm_sas,
            scrin_accept,
            scrin_reject,
            scrin_set_permission,
            scrin_end_session,
            scrin_send_input,
            scrin_send_keys,
            scrin_set_quality,
            scrin_list_trusted,
            scrin_remove_trusted,
            scrin_trust_peer,
            scrin_video_rect,
            scrin_quit,
            scrin_get_server,
            scrin_set_server,
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "scrin failed to start");
            return;
        }
    };
    // `--agent`: started by scrin-service into the console session; it lives
    // in the tray like an autostart.
    if std::env::args().any(|a| a == "--hidden" || a == "--agent")
        && let Some(w) = app.get_webview_window("main")
    {
        let _ = w.hide();
    }
    app.run(|handle, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event
            && code.is_none()
            && !handle
                .try_state::<AppState>()
                .is_some_and(|s| s.quitting.load(Ordering::Acquire))
        {
            // Last window hidden is not a quit; only the tray Quit is.
            api.prevent_exit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::valid_server;

    #[test]
    fn server_urls_are_scheme_host_port_only() {
        assert!(valid_server("https://scrin.example.org"));
        assert!(valid_server("http://100.95.246.105:18443"));
        assert!(valid_server("http://localhost:1/"));
        assert!(!valid_server("ftp://x"));
        assert!(!valid_server("https://"));
        assert!(!valid_server("https://a.b/path"));
        assert!(!valid_server("https://user@a.b"));
        assert!(!valid_server("scrin.example.org"));
    }
}
