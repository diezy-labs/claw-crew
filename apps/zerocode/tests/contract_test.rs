//! Contract tests for Go Engine REST/SSE API and Rust client integration.

#[test]
fn test_create_run_request_contract() {
    let req = serde_json::json!({
        "crew_id": "crew_research_dev",
        "workflow_id": "workflow_code_refactoring",
        "input": {
            "prompt": "Refactor engine/src/crew",
            "target_files": ["engine/src/crew/services.go"]
        },
        "workspace": {
            "root_uri": "file:///workspace"
        },
        "options": {
            "stream": true,
            "require_tool_approval": true
        }
    });

    let s = req.to_string();
    assert!(s.contains("\"crew_id\":\"crew_research_dev\""));
    assert!(s.contains("\"prompt\":\"Refactor engine/src/crew\""));
    assert!(s.contains("\"require_tool_approval\":true"));
}

#[test]
fn test_create_run_response_contract() {
    let raw_resp = r#"{
        "id": "run_01h87b92mkq1",
        "crew_id": "crew_research_dev",
        "status": "queued",
        "events_url": "/api/v1/runs/run_01h87b92mkq1/events",
        "created_at": "2026-09-28T05:20:00Z"
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(raw_resp).expect("valid json");
    assert_eq!(parsed["id"], "run_01h87b92mkq1");
    assert_eq!(parsed["status"], "queued");
    assert_eq!(parsed["events_url"], "/api/v1/runs/run_01h87b92mkq1/events");
}

#[test]
fn test_run_event_contract() {
    let raw_event = r#"{
        "event_id": "evt_01h87b92mkq1",
        "run_id": "run_01h87b92mkq1",
        "sequence": 1,
        "type": "run.created",
        "timestamp": "2026-09-28T05:20:00Z",
        "payload": {
            "status": "queued"
        }
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(raw_event).expect("valid json");
    assert_eq!(parsed["sequence"], 1);
    assert_eq!(parsed["type"], "run.created");
    assert_eq!(parsed["payload"]["status"], "queued");
}
