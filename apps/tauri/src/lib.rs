//! ClawCrew Desktop — Tauri application library.

pub mod capabilities;
pub mod commands;
pub mod daemon;
pub mod gateway_client;
pub mod health;
pub mod macos;
pub mod state;
pub mod tray;

use gateway_client::GatewayClient;
use state::shared_state;
use tauri::{Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Status the splash listens for (`clawcrew://splash-status`). Drives the
/// splash copy when we're starting our own daemon or hit a problem; the happy
/// path is covered by the splash's own health polling, so a missed event is
/// harmless.
#[derive(Clone, serde::Serialize)]
struct SplashStatus {
    /// `starting` | `error` | `missing`.
    kind: &'static str,
    message: String,
}

/// Ensure a gateway/daemon is reachable: reuse one if it already answers,
/// otherwise launch a fresh `clawcrew daemon`. The splash window's health
/// polling takes over once the daemon is up and opens the dashboard.
async fn ensure_daemon<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: state::SharedState) {
    // Serialize daemon lifecycle transitions behind a single lock so a rapid
    // double-toggle or a concurrent single-instance re-launch can't spawn two
    // daemons (or race a stop against a start).
    let spawn_lock = state.read().await.daemon_spawn_lock.clone();
    let _guard = spawn_lock.lock().await;
    ensure_daemon_locked(app, state).await;
}

/// Body of [`ensure_daemon`] that assumes the daemon spawn lock is already held.
async fn ensure_daemon_locked<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: state::SharedState,
) {
    if !state.read().await.service_enabled {
        return;
    }
    let url = {
        let s = state.read().await;
        s.gateway_url.clone()
    };
    let client = GatewayClient::new(&url, None);

    // Give an already-running gateway/daemon a moment to answer before we
    // decide nothing is there — avoids racing a daemon that's mid-startup.
    for _ in 0..3 {
        if client.get_health().await.unwrap_or(false) {
            return; // Reuse the existing instance.
        }
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    }

    // Nothing listening — start our own daemon.
    match daemon::find_clawcrew_binary() {
        Some(bin) => {
            let _ = app.emit(
                "clawcrew://splash-status",
                SplashStatus {
                    kind: "starting",
                    message: "Starting the ClawCrew daemon…".to_string(),
                },
            );
            let port = state::gateway_port_from_url(&url);
            match daemon::spawn_daemon(&bin, port) {
                Ok(child) => {
                    let mut child = Some(child);
                    let should_stop = {
                        let current = state.write().await;
                        if current.service_enabled {
                            *current
                                .owned_daemon
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = child.take();
                            false
                        } else {
                            true
                        }
                    };
                    if should_stop {
                        let mut child = child.expect("newly spawned daemon must be available");
                        let _ = daemon::stop_daemon(&mut child);
                    }
                }
                Err(e) => {
                    let _ = app.emit(
                        "clawcrew://splash-status",
                        SplashStatus {
                            kind: "error",
                            message: format!("Couldn't start the ClawCrew daemon: {e}"),
                        },
                    );
                }
            }
            // On success the splash's health poll detects the daemon and
            // calls `open_dashboard`.
        }
        None => {
            let _ = app.emit(
                "clawcrew://splash-status",
                SplashStatus {
                    kind: "missing",
                    message: "Couldn't find the `clawcrew` binary. Install ClawCrew \
                              (or start a daemon yourself) and reopen the app."
                        .to_string(),
                },
            );
        }
    }

    // Ensure the Go Agent Engine sidecar is running
    if let Some(agent_bin) = daemon::find_agent_engine_binary() {
        let (grpc_port, metrics_port) = {
            let s = state.read().await;
            (s.agent_engine_grpc_port, s.agent_engine_metrics_port)
        };
        if let Ok(child) = daemon::spawn_agent_engine(&agent_bin, grpc_port, metrics_port) {
            let current = state.write().await;
            *current.owned_agent_engine.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        }
    }
}

pub(crate) async fn toggle_service<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: state::SharedState,
) {
    // Hold the daemon spawn lock for the whole flip + (optional) start/stop so a
    // rapid double-toggle can't interleave `ensure_daemon` and spawn two daemons
    // or stop the wrong one.
    let spawn_lock = state.read().await.daemon_spawn_lock.clone();
    let _guard = spawn_lock.lock().await;

    let (service_enabled, daemon_to_stop, agent_to_stop) = {
        let mut current = state.write().await;
        current.service_enabled = !current.service_enabled;
        let daemon_to_stop = if current.service_enabled {
            None
        } else {
            current
                .owned_daemon
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        };
        let agent_to_stop = if current.service_enabled {
            None
        } else {
            current
                .owned_agent_engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        };
        (current.service_enabled, daemon_to_stop, agent_to_stop)
    };

    if let Some(mut child) = daemon_to_stop {
        let _ = daemon::stop_daemon(&mut child);
    }
    if let Some(mut child) = agent_to_stop {
        let _ = daemon::stop_agent_engine(&mut child);
    }
    if service_enabled {
        ensure_daemon_locked(app, state).await;
    }
}

#[tauri::command]
async fn get_service_status(state: tauri::State<'_, state::SharedState>) -> Result<bool, String> {
    Ok(state.read().await.service_enabled)
}

#[tauri::command]
async fn toggle_service_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, state::SharedState>,
) -> Result<bool, String> {
    let shared = state.inner().clone();
    toggle_service(app, shared.clone()).await;
    Ok(shared.read().await.service_enabled)
}

/// Attempt to auto-pair with the gateway so the WebView has a valid token
/// before the React frontend mounts. Runs on localhost so the admin endpoints
/// are accessible without auth.
async fn auto_pair(state: &state::SharedState) -> Option<String> {
    let url = {
        let s = state.read().await;
        s.gateway_url.clone()
    };

    let client = GatewayClient::new(&url, None);

    // Check if gateway is reachable and requires pairing.
    if !client.requires_pairing().await.unwrap_or(false) {
        return None; // Pairing disabled — no token needed.
    }

    // Check if we already have a valid token in state.
    {
        let s = state.read().await;
        if let Some(ref token) = s.token {
            let authed = GatewayClient::new(&url, Some(token));
            if authed.validate_token().await.unwrap_or(false) {
                return Some(token.clone()); // Existing token is valid.
            }
        }
    }

    // No valid token — auto-pair by requesting a new code and exchanging it.
    let client = GatewayClient::new(&url, None);
    match client.auto_pair().await {
        Ok(token) => {
            let mut s = state.write().await;
            s.token = Some(token.clone());
            Some(token)
        }
        Err(_) => None, // Gateway may not be ready yet; health poller will retry.
    }
}

#[tauri::command]
async fn open_dashboard(
    app: tauri::AppHandle,
    state: tauri::State<'_, state::SharedState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let base = {
        let s = state.read().await;
        s.gateway_url.clone()
    };
    let token = auto_pair(state.inner()).await;

    let dashboard_url = format!("{}/", base.trim_end_matches('/'));
    let parsed = tauri::Url::parse(&dashboard_url).map_err(|e| e.to_string())?;

    let mut builder = WebviewWindowBuilder::new(&app, "main", WebviewUrl::External(parsed))
        .title("ClawCrew")
        .inner_size(1200.0, 800.0)
        .center()
        .resizable(true);
    if let Some(token) = token {
        let escaped = token.replace('\\', "\\\\").replace('\'', "\\'");
        let script = format!(
            "try {{ localStorage.setItem('clawcrew_token', '{escaped}'); }} catch (e) {{}}"
        );
        builder = builder.initialization_script(script.as_str());
    }
    builder.build().map_err(|e| e.to_string())?;

    // Hand off from the splash to the dashboard.
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
    Ok(())
}

/// Set the macOS dock icon programmatically so it shows even in dev builds
/// (which don't have a proper .app bundle).
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::NSApplication;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSData;

    let icon_bytes = include_bytes!("../icons/128x128.png");
    // Safety: setup() runs on the main thread in Tauri.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let data = NSData::with_bytes(icon_bytes);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: `mtm` proves this code is running on the AppKit main thread,
        // and both `app` and `image` are live Objective-C objects for the
        // duration of the message send.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

/// Configure and run the Tauri application.
pub fn run() {
    let shared = shared_state();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second shortcut launch must revive the existing instance rather
            // than leave a failed splash waiting forever in the background.
            let target = app
                .get_webview_window("main")
                .or_else(|| app.get_webview_window("splash"));
            if let Some(window) = target {
                let _ = window.show();
                let _ = window.set_focus();
            }

            let state = app.state::<state::SharedState>().inner().clone();
            tauri::async_runtime::spawn(ensure_daemon(app.clone(), state));
        }))
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            commands::gateway::get_status,
            commands::gateway::get_health,
            commands::channels::list_channels,
            commands::pairing::initiate_pairing,
            commands::pairing::get_devices,
            commands::agent::send_message,
            commands::engine::get_engine_metrics,
            commands::engine::get_engine_logs,
            commands::engine::get_engine_health,
            commands::engine::start_agent_turn,
            commands::engine::query_agent_memory,
            open_dashboard,
            get_service_status,
            toggle_service_command,
            capabilities::screenshot::take_screenshot,
            capabilities::applescript::run_applescript,
        ])
        .setup(move |app| {
            // Set macOS dock icon (needed for dev builds without .app bundle).
            #[cfg(target_os = "macos")]
            set_dock_icon();

            // Set up the system tray.
            let _ = tray::setup_tray(app);

            // Reflect the initial service state on the tray menu (enabled by
            // default) before the health poller takes over.
            tray::sync_service_menu(true, false);

            // Show the splash window on launch. It polls the gateway for
            // readiness and then asks the backend to open the dashboard
            // (`open_dashboard`) pointed at the running web gateway — which
            // takes a first-time user straight into the Quickstart.
            if let Some(splash) = app.get_webview_window("splash") {
                let _ = splash.show();
                let _ = splash.set_focus();
            }

            // Reuse a running gateway/daemon, or start a fresh `clawcrew daemon`
            // if none is listening, so the app works without a manual setup step.
            let ensure_handle = app.handle().clone();
            let ensure_state = shared.clone();
            tauri::async_runtime::spawn(ensure_daemon(ensure_handle, ensure_state));

            // Start background health polling (drives the tray icon/tooltip).
            health::spawn_health_poller(app.handle().clone(), shared.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                // Closing the dashboard hides the app; the tray and daemon
                // remain available, and the shortcut can show it again.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Keep the app running in the background when all windows are closed.
            // This is the standard pattern for menu bar / tray apps.
            if let RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
