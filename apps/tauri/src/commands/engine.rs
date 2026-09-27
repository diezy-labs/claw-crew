//! Tauri commands for interacting with the Go 1.27 Agent Engine sidecar.

use crate::state::SharedState;
use std::path::PathBuf;
use tauri::State;

/// Retrieve raw Prometheus metrics from the Go Agent Engine HTTP endpoint.
#[tauri::command]
pub async fn get_engine_metrics(state: State<'_, SharedState>) -> Result<String, String> {
    let port = {
        let s = state.read().await;
        s.agent_engine_metrics_port
    };
    let url = format!("http://127.0.0.1:{port}/metrics");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach engine metrics at {url}: {e}"))?;

    resp.text()
        .await
        .map_err(|e| format!("Failed to read engine metrics response: {e}"))
}

/// Retrieve the latest log lines from the central agent.log file.
#[tauri::command]
pub async fn get_engine_logs(lines: Option<usize>) -> Result<Vec<String>, String> {
    let limit = lines.unwrap_or(100);
    let log_path = resolve_agent_log_path();

    if !log_path.exists() {
        return Ok(vec![format!("Log file not found at: {}", log_path.display())]);
    }

    let file = std::fs::File::open(&log_path)
        .map_err(|e| format!("Failed to open log file {}: {e}", log_path.display()))?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;

    let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let start = if all_lines.len() > limit {
        all_lines.len() - limit
    } else {
        0
    };

    Ok(all_lines[start..].to_vec())
}

/// Check health of the Go Agent Engine sidecar.
#[tauri::command]
pub async fn get_engine_health(state: State<'_, SharedState>) -> Result<bool, String> {
    let port = {
        let s = state.read().await;
        s.agent_engine_metrics_port
    };
    let url = format!("http://127.0.0.1:{port}/healthz");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(800))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    match client.get(&url).send().await {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}

/// Trigger an agent turn on the Go Agent Engine and return the streamed turn response.
#[tauri::command]
pub async fn start_agent_turn(
    state: State<'_, SharedState>,
    session_id: String,
    agent_id: Option<String>,
    prompt: String,
) -> Result<serde_json::Value, String> {
    let port = {
        let s = state.read().await;
        s.agent_engine_metrics_port
    };
    let url = format!("http://127.0.0.1:{port}/api/turn");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let payload = serde_json::json!({
        "session_id": session_id,
        "agent_id": agent_id.unwrap_or_else(|| "primary_agent".to_string()),
        "prompt": prompt,
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to execute agent turn at {url}: {e}"))?;

    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read turn response: {e}"))?;

    Ok(serde_json::json!({
        "status": "success",
        "raw_stream": text,
    }))
}

/// Query local in-memory vector store on the Go Agent Engine.
#[tauri::command]
pub async fn query_agent_memory(
    state: State<'_, SharedState>,
    query: String,
    top_k: Option<i32>,
) -> Result<serde_json::Value, String> {
    let port = {
        let s = state.read().await;
        s.agent_engine_metrics_port
    };
    let url = format!("http://127.0.0.1:{port}/api/query");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let payload = serde_json::json!({
        "query": query,
        "top_k": top_k.unwrap_or(5),
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to query agent memory at {url}: {e}"))?;

    let results: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse query memory JSON response: {e}"))?;

    Ok(results)
}

/// Resolve the expected platform-specific location of agent.log.
fn resolve_agent_log_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("clawcrew").join("logs").join("agent.log");
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".clawcrew").join("logs").join("agent.log");
    }

    PathBuf::from("logs").join("agent.log")
}
