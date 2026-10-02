// Tauri v2 Native Bridge & Desktop Scaffolding Utilities

export interface TauriWindowConfig {
  title: string;
  width: number;
  height: number;
  resizable: boolean;
  fullscreen: boolean;
  decorations: boolean;
  transparent: boolean;
}

export const isTauriEnvironment = (): boolean => {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
};

export const TAURI_V2_CONFIG_JSON = `{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Galleon Sovereign",
  "version": "2.4.0",
  "identifier": "com.galleon.sovereign",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:3000",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Galleon Sovereign — Autonomous Fleet AI",
        "width": 1280,
        "height": 840,
        "minWidth": 900,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false,
        "decorations": true,
        "transparent": false,
        "theme": "Dark",
        "dragDropEnabled": true
      }
    ],
    "trayIcon": {
      "iconPath": "icons/icon.png",
      "tooltip": "Galleon Sovereign Deck",
      "iconAsTemplate": true
    },
    "security": {
      "csp": "default-src 'self'; script-src 'self' 'unsafe-eval' 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' https: ipc:;"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "category": "DeveloperTool",
    "shortDescription": "Autonomous Local-First Fleet Orchestration & Multi-Agent IDE",
    "longDescription": "Sovereign AI development environment with decentralized agent crew coordination."
  },
  "plugins": {
    "fs": {},
    "dialog": {},
    "notification": {},
    "shell": {
      "open": true
    }
  }
}`;

export const TAURI_V2_CARGO_TOML = `[package]
name = "galleon-sovereign"
version = "2.4.0"
description = "Sovereign Autonomous Fleet AI Desktop Environment"
authors = ["Captain & Sovereign Crew"]
edition = "2021"

[lib]
name = "galleon_sovereign_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.0", features = [] }

[dependencies]
tauri = { version = "2.0", features = ["tray-icon", "image-ico", "image-png"] }
tauri-plugin-shell = "2.0"
tauri-plugin-dialog = "2.0"
tauri-plugin-fs = "2.0"
tauri-plugin-notification = "2.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.36", features = ["full"] }
`;

export const TAURI_V2_MAIN_RS = `// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct FleetMetricResponse {
    pub active_ships: u32,
    pub assigned_crew: u32,
    pub running_voyages: u32,
    pub status: String,
}

#[tauri::command]
fn get_fleet_metrics() -> FleetMetricResponse {
    FleetMetricResponse {
        active_ships: 3,
        assigned_crew: 8,
        running_voyages: 2,
        status: "Sovereign & Anchored".into(),
    }
}

#[tauri::command]
fn ring_deck_bell(app: AppHandle) -> String {
    println!("[TAURI-IPC] Ship Bell chimed by Sovereign Captain");
    "Bell chimed successfully".into()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![get_fleet_metrics, ring_deck_bell])
        .setup(|app| {
            // Build Tray Menu
            let quit_i = MenuItem::with_id(app, "quit", "Quit Galleon Sovereign", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Open Quarterdeck", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Galleon Sovereign desktop application");
}
`;

export const TAURI_V2_CAPABILITIES_JSON = `{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default permissions for Galleon Sovereign v2",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open",
    "dialog:default",
    "fs:default",
    "notification:default"
  ]
}`;
