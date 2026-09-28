//! Go Agent Engine REST and SSE client.
//! Connects the Rust Zerocode presentation layer to the Go 1.27 AI Brain & Execution Engine.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunRequestDto {
    pub crew_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    pub input: RunInputDto,
    pub workspace: WorkspaceConfigDto,
    pub options: RunOptionsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInputDto {
    pub prompt: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub target_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfigDto {
    pub root_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOptionsDto {
    pub stream: bool,
    pub require_tool_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunResponseDto {
    pub id: String,
    pub crew_id: String,
    pub status: String,
    pub events_url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatusDto {
    pub id: String,
    pub crew_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEventDto {
    pub event_id: String,
    pub run_id: String,
    pub sequence: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct EngineClient {
    base_url: String,
    client: reqwest::Client,
}

impl EngineClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn start_run(
        &self,
        crew_id: &str,
        prompt: &str,
        target_files: Vec<String>,
        workspace_root: &str,
    ) -> Result<CreateRunResponseDto> {
        let url = format!("{}/api/v1/runs", self.base_url);
        let req = CreateRunRequestDto {
            crew_id: crew_id.to_string(),
            workflow_id: None,
            input: RunInputDto {
                prompt: prompt.to_string(),
                target_files,
            },
            workspace: WorkspaceConfigDto {
                root_uri: workspace_root.to_string(),
            },
            options: RunOptionsDto {
                stream: true,
                require_tool_approval: true,
            },
        };

        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("failed to send start_run request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("start_run failed with status {}: {}", status, body);
        }

        resp.json::<CreateRunResponseDto>()
            .await
            .context("failed to parse start_run response")
    }

    pub async fn get_run(&self, run_id: &str) -> Result<RunStatusDto> {
        let url = format!("{}/api/v1/runs/{}", self.base_url, run_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send get_run request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("get_run failed with status {}: {}", status, body);
        }

        resp.json::<RunStatusDto>()
            .await
            .context("failed to parse get_run response")
    }

    pub async fn cancel_run(&self, run_id: &str) -> Result<()> {
        let url = format!("{}/api/v1/runs/{}/cancel", self.base_url, run_id);
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .context("failed to send cancel_run request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("cancel_run failed with status {}: {}", status, body);
        }

        Ok(())
    }

    pub async fn respond_approval(&self, approval_id: &str, approved: bool, reason: &str) -> Result<()> {
        let url = format!("{}/api/v1/approvals", self.base_url);
        let payload = serde_json::json!({
            "approval_id": approval_id,
            "approved": approved,
            "reason": reason,
        });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("failed to send respond_approval request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("respond_approval failed with status {}: {}", status, body);
        }

        Ok(())
    }

    pub async fn get_artifact(&self, artifact_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/artifacts/{}", self.base_url, artifact_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send get_artifact request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("get_artifact failed with status {}: {}", status, body);
        }

        resp.json::<serde_json::Value>()
            .await
            .context("failed to parse get_artifact response")
    }

    pub async fn get_artifact_diff(&self, artifact_id: &str) -> Result<String> {
        let url = format!("{}/api/v1/artifacts/{}/diff", self.base_url, artifact_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send get_artifact_diff request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("get_artifact_diff failed with status {}: {}", status, body);
        }

        let val: serde_json::Value = resp.json().await.context("failed to parse diff response")?;
        Ok(val.get("diff").and_then(|d| d.as_str()).unwrap_or_default().to_string())
    }

    pub async fn list_workflows(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/workflows", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send list_workflows request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list_workflows failed with status {}: {}", status, body);
        }

        resp.json::<serde_json::Value>()
            .await
            .context("failed to parse list_workflows response")
    }

    pub async fn instantiate_workflow(&self, workflow_id: &str, run_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/workflows/{}/instantiate", self.base_url, workflow_id);
        let payload = serde_json::json!({
            "run_id": run_id,
        });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("failed to send instantiate_workflow request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("instantiate_workflow failed with status {}: {}", status, body);
        }

        resp.json::<serde_json::Value>()
            .await
            .context("failed to parse instantiate_workflow response")
    }
}
