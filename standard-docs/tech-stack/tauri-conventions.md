# Tauri Conventions

**Status:** Normative (code MUST follow this)  
**Last updated:** 2026-10-03  
**Tauri version:** v2

---

## Project Structure

```
apps/tauri-2/
├── src/
│   ├── main.rs              # Entrypoint, command registration
│   ├── commands.rs          # Tauri IPC commands
│   └── setup.rs             # App setup, window config
├── src-tauri/
│   ├── tauri.conf.json      # Tauri config (allowlist, bundle)
│   ├── icons/               # App icons (platform-specific)
│   └── Cargo.toml
├── dist/                    # Built web-2 bundle (copied at build)
└── README.md
```

**Convention:** `src/` = Rust backend (IPC commands), `dist/` = frontend bundle (React).

---

## IPC Command Patterns

### Define Commands

```rust
// src/commands.rs
use tauri::command;

#[command]
pub async fn get_fleet_metrics() -> Result<FleetMetrics, String> {
    // Call Go engine HTTP :9090 (NOT Rust gRPC :50052)
    let response = reqwest::get("http://localhost:9090/api/system/metrics")
        .await
        .map_err(|e| e.to_string())?;
    
    let metrics: FleetMetrics = response.json()
        .await
        .map_err(|e| e.to_string())?;
    
    Ok(metrics)
}

#[command]
pub async fn start_crew_turn(crew_id: String, task: String) -> Result<TurnResponse, String> {
    let client = reqwest::Client::new();
    let response = client.post("http://localhost:9090/api/crews/run")
        .json(&serde_json::json!({
            "crew_id": crew_id,
            "task": task,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    response.json().await.map_err(|e| e.to_string())
}

#[command]
pub fn open_logs_folder() -> Result<(), String> {
    let logs_path = std::env::var("APPDATA")
        .map(|appdata| format!("{}\\galleon\\logs", appdata))
        .unwrap_or_else(|_| "./logs".to_string());
    
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&logs_path)
        .spawn()
        .map_err(|e| e.to_string())?;
    
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&logs_path)
        .spawn()
        .map_err(|e| e.to_string())?;
    
    Ok(())
}
```

**Conventions:**
- Commands are `async fn` if they do I/O (HTTP, filesystem)
- Return `Result<T, String>` (Tauri serializes `Err(String)` to frontend)
- Error messages are user-facing (not debug output)
- Commands call Go HTTP :9090 (NOT Rust gRPC :50052, which is internal)

### Register Commands

```rust
// src/main.rs
use tauri::Builder;
mod commands;

fn main() {
    Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_fleet_metrics,
            commands::start_crew_turn,
            commands::open_logs_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Convention:** All commands registered in `generate_handler![]` (compile-time allowlist).

---

## Frontend IPC Calls

### TypeScript Client

```typescript
// web-2/src/lib/tauri.ts
import { invoke } from '@tauri-apps/api/core';

export interface FleetMetrics {
  gateway_latency_ms: number;
  memory_db_mb: number;
  active_crews: number;
}

export async function getFleetMetrics(): Promise<FleetMetrics> {
  return await invoke<FleetMetrics>('get_fleet_metrics');
}

export async function startCrewTurn(crewId: string, task: string): Promise<TurnResponse> {
  return await invoke<TurnResponse>('start_crew_turn', { crewId, task });
}

export async function openLogsFolder(): Promise<void> {
  await invoke('open_logs_folder');
}
```

**Conventions:**
- `invoke<T>('command_name', { args })` for typed calls
- Snake_case in Rust → camelCase in TypeScript (Tauri auto-converts)
- Async/await (all IPC is async)

### React Component Usage

```typescript
// web-2/src/components/SystemMetrics.tsx
import { useEffect, useState } from 'react';
import { getFleetMetrics, type FleetMetrics } from '@/lib/tauri';

export function SystemMetrics() {
  const [metrics, setMetrics] = useState<FleetMetrics | null>(null);
  const [error, setError] = useState<string | null>(null);
  
  useEffect(() => {
    getFleetMetrics()
      .then(setMetrics)
      .catch((err) => setError(err.toString()));
  }, []);
  
  if (error) return <div>Error: {error}</div>;
  if (!metrics) return <div>Loading...</div>;
  
  return (
    <div>
      <p>Latency: {metrics.gateway_latency_ms}ms</p>
      <p>Memory: {metrics.memory_db_mb}MB</p>
    </div>
  );
}
```

---

## Security Allowlist

### Config: `tauri.conf.json`

```json
{
  "app": {
    "security": {
      "csp": "default-src 'self'; connect-src 'self' http://localhost:9090"
    }
  },
  "allowlist": {
    "all": false,
    "fs": {
      "scope": ["$APPDATA/galleon/*", "$LOCALDATA/galleon/*"],
      "readDir": true,
      "writeFile": true
    },
    "shell": {
      "open": true,
      "scope": [
        { "name": "explorer", "cmd": "explorer", "args": true },
        { "name": "open", "cmd": "open", "args": true }
      ]
    },
    "http": {
      "scope": ["http://localhost:9090/*"]
    }
  }
}
```

**Conventions:**
- `all: false` (deny-by-default, explicit grants)
- `fs.scope` limits file access to app data dirs (`$APPDATA`, `$LOCALDATA`)
- `shell.scope` allowlists specific binaries (no arbitrary command execution)
- `http.scope` limits network calls to Go engine :9090 (no external URLs)

---

## Window Lifecycle

### Setup: `src/setup.rs`

```rust
use tauri::{Manager, WindowBuilder};

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Create main window
    let window = WindowBuilder::new(app, "main", tauri::WindowUrl::App("index.html".into()))
        .title("Galleon Fleet")
        .min_inner_size(1024.0, 768.0)
        .center()
        .build()?;
    
    // Restore window state from disk
    if let Ok(state) = load_window_state() {
        window.set_size(state.size)?;
        window.set_position(state.position)?;
    }
    
    // Save state on close
    let window_clone = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            if let Ok(size) = window_clone.inner_size() {
                let _ = save_window_state(WindowState { size, position: window_clone.outer_position().unwrap() });
            }
        }
    });
    
    Ok(())
}
```

**Convention:** Save/restore window state to `$APPDATA/galleon/window.json` (persistent UX).

### System Tray

```rust
use tauri::{CustomMenuItem, SystemTray, SystemTrayMenu, SystemTrayEvent};

pub fn build_tray() -> SystemTray {
    let tray_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("show", "Show Window"))
        .add_item(CustomMenuItem::new("quit", "Quit"));
    
    SystemTray::new().with_menu(tray_menu)
}

pub fn handle_tray_event(app: &tauri::AppHandle, event: SystemTrayEvent) {
    match event {
        SystemTrayEvent::MenuItemClick { id, .. } => {
            match id.as_str() {
                "show" => {
                    if let Some(window) = app.get_window("main") {
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                }
                "quit" => {
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        _ => {}
    }
}
```

**Convention:** Tray icon + menu for background mode (minimize to tray, not taskbar).

---

## Boundary Policy (NO Business Logic)

### ✅ Correct: Proxy to Go Engine

```rust
#[command]
pub async fn get_crew_roster() -> Result<Vec<Crew>, String> {
    // Tauri = thin proxy, Go = SSOT
    let response = reqwest::get("http://localhost:9090/api/fleet/crews")
        .await
        .map_err(|e| e.to_string())?;
    
    response.json().await.map_err(|e| e.to_string())
}
```

### ❌ Wrong: Business Logic in Tauri

```rust
#[command]
pub async fn should_auto_approve(risk: &str) -> Result<bool, String> {
    // ❌ Policy logic belongs to Go engine, not Tauri
    Ok(risk == "low" || risk == "minor")
}
```

### ❌ Wrong: Fake Data (ADR 0003 Violation)

```rust
#[command]
pub fn get_system_metrics() -> FleetMetrics {
    // ❌ Hardcoded fake data, should read from Go API
    FleetMetrics {
        gateway_latency_ms: 8,
        memory_db_mb: 18.4,
    }
}
```

**Rule:** Tauri commands MUST delegate to Go HTTP :9090 (no domain logic, no fake data).

---

## Error Handling

### User-Facing Errors

```rust
#[command]
pub async fn start_crew_turn(crew_id: String, task: String) -> Result<TurnResponse, String> {
    let response = reqwest::Client::new()
        .post("http://localhost:9090/api/crews/run")
        .json(&serde_json::json!({ "crew_id": crew_id, "task": task }))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to engine: {}", e))?; // User-facing error
    
    if !response.status().is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(format!("Engine returned error: {}", error_body));
    }
    
    response.json().await.map_err(|e| format!("Invalid response format: {}", e))
}
```

**Conventions:**
- `map_err(|e| format!("User message: {}", e))` for user-facing errors
- Include context ("Failed to connect", not just "error")
- Frontend displays `Err(String)` directly in UI

---

## Platform-Specific Code

### Conditional Compilation

```rust
#[command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    
    Ok(())
}
```

**Convention:** Use `#[cfg(target_os = "...")]` for platform-specific logic (file paths, shell commands).

---

## Build & Bundle

### Dev Mode

```bash
cd apps/tauri-2
cargo tauri dev
```

**Runs:** Vite dev server (:5173) + Tauri shell (hot reload enabled)

### Production Build

```bash
cd apps/tauri-2
cargo tauri build --target x86_64-pc-windows-msvc  # Windows .exe
cargo tauri build --target x86_64-apple-darwin     # macOS .dmg
cargo tauri build --target x86_64-unknown-linux    # Linux .AppImage
```

**Output:** `target/release/bundle/` (platform-specific installers)

### Bundle Config: `tauri.conf.json`

```json
{
  "bundle": {
    "identifier": "com.galleon.fleet",
    "icon": ["icons/icon.png"],
    "active": true,
    "targets": ["msi", "nsis"],
    "windows": {
      "wix": {
        "language": ["en-US"]
      }
    }
  }
}
```

**Convention:** `identifier` = reverse domain (`com.galleon.fleet`), icons in `icons/` (multi-res).

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_get_fleet_metrics_success() {
        // Mock HTTP server
        let mock_server = mockito::Server::new();
        let mock = mock_server.mock("GET", "/api/system/metrics")
            .with_status(200)
            .with_body(r#"{"gateway_latency_ms":12,"memory_db_mb":20.5}"#)
            .create();
        
        // Call command (override URL to mock server)
        let result = get_fleet_metrics_internal(&mock_server.url()).await;
        
        mock.assert();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().gateway_latency_ms, 12);
    }
}
```

**Convention:** Use `mockito` to mock HTTP calls (no real Go engine in unit tests).

### E2E Tests (WebDriver)

```rust
// tests/e2e.rs
#[test]
fn test_app_opens() {
    let app = tauri::test::mock_app();
    let window = app.get_window("main").unwrap();
    
    assert!(window.is_visible().unwrap());
}
```

**Convention:** E2E tests in `tests/` (requires `--features e2e` in `Cargo.toml`).

---

## Performance

### Async Commands (Non-Blocking)

```rust
// ✅ Correct: async command (doesn't block main thread)
#[command]
pub async fn fetch_crew_history(crew_id: String) -> Result<Vec<Message>, String> {
    let response = reqwest::get(format!("http://localhost:9090/api/crews/{}/history", crew_id))
        .await
        .map_err(|e| e.to_string())?;
    
    response.json().await.map_err(|e| e.to_string())
}

// ❌ Wrong: sync command (blocks main thread on I/O)
#[command]
pub fn fetch_crew_history_sync(crew_id: String) -> Result<Vec<Message>, String> {
    let response = reqwest::blocking::get(format!("http://localhost:9090/api/crews/{}/history", crew_id))
        .map_err(|e| e.to_string())?; // Blocks!
    
    response.json().map_err(|e| e.to_string())
}
```

**Convention:** All I/O commands are `async fn` (Tauri runs them on thread pool).

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What Tauri owns vs delegates
- [ADR 0003: Tauri Metrics Real API](../decisions/0003-tauri-metrics-real-api.md) — No fake data
- [Web Conventions](./web-conventions.md) — Frontend patterns (React, Tailwind)
