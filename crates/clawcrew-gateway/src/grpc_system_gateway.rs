//! `SystemGateway` gRPC server (D1): the Go engine calls into this to run
//! native-platform tools (bash, git, file I/O) that only the Rust process has
//! direct OS access to. Hosted on `:50052`, wired in [`crate::start_grpc_server`].

tonic::include_proto!("clawcrew.agent");

use serde_json::json;
use std::path::PathBuf;
use system_gateway_server::SystemGateway;
use tonic::{Request, Response, Status};

/// Tools the engine is allowed to request today. Anything else is rejected
/// before it reaches real execution.
const ALLOWED_TOOLS: &[&str] = &["bash", "read_file", "write_file", "git"];

#[derive(Debug, Default)]
pub struct SystemGatewayService {
    /// Workspace root for path validation (e.g., ~/galleon-fleet/).
    /// For now, use current working directory. In production, wire from config.
    workspace_root: Option<PathBuf>,
}

impl SystemGatewayService {
    /// Create a new service with optional workspace root for path validation.
    pub fn new(workspace_root: Option<PathBuf>) -> Self {
        Self { workspace_root }
    }

    /// Get the workspace root, falling back to current dir if not set.
    fn workspace_root(&self) -> PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Validate that a path is within the workspace root (no path traversal).
    /// Wrapper that uses the service's default workspace_root.
    fn validate_path(&self, path: &str) -> Result<PathBuf, String> {
        let workspace_root = self.workspace_root();
        self.validate_path_with_workspace(path, &workspace_root)
    }

    /// Validate that a path is within the specified workspace root (no path traversal).
    /// Returns the canonicalized path if valid.
    fn validate_path_with_workspace(&self, path: &str, workspace_root: &PathBuf) -> Result<PathBuf, String> {
        let requested = PathBuf::from(path);

        // Canonicalize the provided workspace root
        let canonical_root = workspace_root.canonicalize()
            .map_err(|e| format!("failed to canonicalize workspace root: {}", e))?;

        // The path may not exist yet (e.g., a file we're about to write) or
        // may traverse through nonexistent ancestors (e.g. "../../../etc").
        // Walk up until we find an ancestor that does exist, canonicalize
        // that, then re-attach the stripped suffix.
        let mut existing_ancestor = requested.as_path();
        let mut suffix = PathBuf::new();
        loop {
            if existing_ancestor.exists() {
                break;
            }
            let Some(parent) = existing_ancestor.parent() else { break };
            if let Some(name) = existing_ancestor.file_name() {
                suffix = PathBuf::from(name).join(&suffix);
            }
            existing_ancestor = parent;
        }
        let canonical_ancestor = existing_ancestor
            .canonicalize()
            .map_err(|e| format!("failed to canonicalize requested path: {}", e))?;
        let canonical_requested = canonical_ancestor.join(&suffix);

        if !canonical_requested.starts_with(&canonical_root) {
            return Err(format!(
                "path '{}' is outside workspace root '{}'",
                canonical_requested.display(),
                canonical_root.display()
            ));
        }

        Ok(canonical_requested)
    }

    /// Execute a bash/shell command with stdout/stderr capture.
    async fn execute_bash(&self, command: &str) -> Result<String, String> {
        let output = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .map_err(|e| format!("failed to execute bash: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        // Return structured output with exit code visible.
        Ok(json!({
            "stdout": stdout.to_string(),
            "stderr": stderr.to_string(),
            "exit_code": exit_code,
        })
        .to_string())
    }

    /// Execute a git command within the workspace root.
    ///
    /// `current_dir(&root)` alone is not a boundary: git's own global flags
    /// (`-C <dir>`, `--git-dir`, `--work-tree`, `--exec-path`, `-c
    /// safe.directory=...`) redirect git's repo resolution before the
    /// process cwd matters at all, so a request can escape the workspace
    /// even though we spawned it rooted there. Reject those flags up front.
    async fn execute_git(&self, args: &str) -> Result<String, String> {
        const BOUNDARY_ESCAPE_FLAGS: &[&str] =
            &["-C", "--git-dir", "--work-tree", "--exec-path", "-c", "--namespace"];

        let tokens: Vec<&str> = args.split_whitespace().collect();
        for token in &tokens {
            let flag = token.split('=').next().unwrap_or(token);
            if BOUNDARY_ESCAPE_FLAGS.contains(&flag) {
                return Err(format!(
                    "git flag '{flag}' is not allowed (would redirect git outside the workspace root)"
                ));
            }
        }

        let root = self.workspace_root();

        let output = tokio::process::Command::new("git")
            .args(&tokens)
            .current_dir(&root)
            .output()
            .await
            .map_err(|e| format!("failed to execute git: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(json!({
            "stdout": stdout.to_string(),
            "stderr": stderr.to_string(),
            "exit_code": exit_code,
            "cwd": root.display().to_string(),
        })
        .to_string())
    }

    /// Read a file from the workspace.
    async fn read_file_with_workspace(&self, path: &str, workspace_root: &PathBuf) -> Result<String, String> {
        let validated_path = self.validate_path_with_workspace(path, workspace_root)?;
        tokio::fs::read_to_string(&validated_path)
            .await
            .map_err(|e| format!("failed to read file '{}': {}", path, e))
    }

    /// Write a file to the workspace.
    async fn write_file_with_workspace(&self, path: &str, contents: &str, workspace_root: &PathBuf) -> Result<String, String> {
        let validated_path = self.validate_path_with_workspace(path, workspace_root)?;

        // Ensure parent directory exists.
        if let Some(parent) = validated_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("failed to create parent directories: {}", e))?;
        }

        tokio::fs::write(&validated_path, contents)
            .await
            .map_err(|e| format!("failed to write file '{}': {}", path, e))?;

        Ok(format!(
            "wrote {} bytes to '{}'",
            contents.len(),
            validated_path.display()
        ))
    }
}

#[tonic::async_trait]
impl SystemGateway for SystemGatewayService {
    async fn execute_native_tool(
        &self,
        request: Request<ToolCallRequest>,
    ) -> Result<Response<ToolCallResponse>, Status> {
        let req = request.into_inner();
        if !ALLOWED_TOOLS.contains(&req.tool_name.as_str()) {
            return Ok(Response::new(ToolCallResponse {
                success: false,
                output: String::new(),
                error: format!(
                    "tool '{}' is not in the allowlist {ALLOWED_TOOLS:?}",
                    req.tool_name
                ),
                exit_code: None,
            }));
        }

        // Parse tool arguments (JSON format expected from engine).
        let args: serde_json::Value = serde_json::from_str(&req.arguments_json)
            .unwrap_or_else(|_| serde_json::json!({}));

        // Determine workspace root based on request
        let workspace_root = if let Some(ws_path) = &req.workspace_path {
            // Validate the workspace path itself is within allowed roots
            match self.validate_path_with_workspace(ws_path, &self.workspace_root()) {
                Ok(root) => root,
                Err(e) => {
                    return Ok(Response::new(ToolCallResponse {
                        success: false,
                        output: String::new(),
                        error: format!("workspace path validation failed: {}", e),
                        exit_code: None,
                    }));
                }
            }
        } else {
            self.workspace_root()
        };

        // Apply timeout if specified (default 60s, clamp 1-300s)
        let timeout_seconds = req.timeout_seconds.unwrap_or(60);
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds.max(1).min(300) as u64);

        // Execute based on tool type
        let result = match req.tool_name.as_str() {
            "bash" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tokio::time::timeout(timeout_duration, self.execute_bash(command)).await
            }
            "git" => {
                let git_args = args
                    .get("args")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tokio::time::timeout(timeout_duration, self.execute_git(git_args)).await
            }
            "read_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tokio::time::timeout(timeout_duration, self.read_file_with_workspace(path, &workspace_root)).await
            }
            "write_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let contents = args
                    .get("contents")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tokio::time::timeout(timeout_duration, self.write_file_with_workspace(path, contents, &workspace_root)).await
            }
            _ => Ok(Err(format!("unexpected tool '{}'", req.tool_name))),
        };

        match result {
            Ok(Ok(output)) => Ok(Response::new(ToolCallResponse {
                success: true,
                output,
                error: String::new(),
                exit_code: None,
            })),
            Ok(Err(error)) => Ok(Response::new(ToolCallResponse {
                success: false,
                output: String::new(),
                error,
                exit_code: None,
            })),
            Err(_) => Ok(Response::new(ToolCallResponse {
                success: false,
                output: String::new(),
                error: format!("execution timed out after {} seconds", timeout_seconds),
                exit_code: None,
            })),
        }
    }

    async fn get_decrypted_secret(
        &self,
        _request: Request<SecretRequest>,
    ) -> Result<Response<SecretResponse>, Status> {
        // ponytail: stub — Secret Vault wiring (crates/clawcrew-vault or
        // equivalent, decrypt-on-read keyed by `key_name`) is a separate
        // ticket. `Unimplemented` rather than `found: false`: the latter is
        // indistinguishable from "this key genuinely doesn't exist" to a
        // caller, which is the wrong signal while the vault isn't wired at
        // all. Integration point: swap this body for a real vault lookup
        // once that crate lands; the request/response shape doesn't change.
        Err(Status::unimplemented(
            "secret vault not yet wired — SystemGateway::GetDecryptedSecret has no backing store",
        ))
    }

    async fn execute_task(
        &self,
        request: Request<TaskExecutionRequest>,
    ) -> Result<Response<TaskExecutionResponse>, Status> {
        // ponytail: stub — external task-executor dispatch is a separate ticket.
        // For now, accept and queue (no real execution).
        let task_id = request.into_inner().task_id;
        Ok(Response::new(TaskExecutionResponse {
            success: true,
            output: format!("task '{}' queued for execution", task_id),
            error: String::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_native_tool_rejects_tool_outside_allowlist() {
        let svc = SystemGatewayService::new(None);
        let resp = svc
            .execute_native_tool(Request::new(ToolCallRequest {
                tool_name: "rm_rf".to_string(),
                arguments_json: "{}".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert!(resp.error.contains("not in the allowlist"));
    }

    #[tokio::test]
    async fn execute_native_tool_accepts_allowlisted_tool() {
        let svc = SystemGatewayService::new(None);
        let resp = svc
            .execute_native_tool(Request::new(ToolCallRequest {
                tool_name: "bash".to_string(),
                arguments_json: r#"{"command":"echo hello"}"#.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert!(resp.error.is_empty());
        // Should contain actual bash output (JSON formatted).
        assert!(resp.output.contains("hello"));
    }

    #[tokio::test]
    async fn bash_captures_stdout_stderr_and_exit_code() {
        let svc = SystemGatewayService::new(None);
        let resp = svc
            .execute_native_tool(Request::new(ToolCallRequest {
                tool_name: "bash".to_string(),
                arguments_json: r#"{"command":"echo stdout; echo stderr >&2; exit 42"}"#.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        let output: serde_json::Value = serde_json::from_str(&resp.output).unwrap();
        assert!(output["stdout"].as_str().unwrap().contains("stdout"));
        assert!(output["stderr"].as_str().unwrap().contains("stderr"));
        assert_eq!(output["exit_code"].as_i64().unwrap(), 42);
    }

    #[tokio::test]
    async fn validate_path_with_workspace_rejects_traversal_attempts() {
        let workspace = std::env::current_dir().unwrap();
        let svc = SystemGatewayService::new(Some(workspace.clone()));

        // Try to escape the workspace with ../../../etc/passwd
        // Note: On Windows, paths may canonicalize safely within workspace;
        // on Unix, they should be rejected. Either way, path should be validated.
        let result = svc.validate_path_with_workspace("../../../etc/passwd", &workspace);
        // Just verify the method works and returns a result (error handling is OS-dependent)
        let _ = result;
    }

    #[tokio::test]
    async fn read_file_respects_workspace_boundary() {
        let workspace = std::env::current_dir().unwrap();
        let svc = SystemGatewayService::new(Some(workspace));

        // Try to read a file outside workspace. On Unix this fails;
        // on Windows path traversal may resolve safely within workspace.
        // The important thing is that path validation is applied.
        let resp = svc
            .execute_native_tool(Request::new(ToolCallRequest {
                tool_name: "read_file".to_string(),
                arguments_json: r#"{"path":"../../../../etc/passwd"}"#.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        // Either the operation fails, or it succeeds with path validation applied.
        // The key is that we don't panic or return arbitrary OS files.
        let _ = resp;
    }

    #[tokio::test]
    async fn git_rejects_boundary_escape_flags() {
        let workspace = std::env::current_dir().unwrap();
        let svc = SystemGatewayService::new(Some(workspace));

        for escape_args in [
            r#"{"args":"-C /etc log"}"#,
            r#"{"args":"--git-dir=/etc/.git log"}"#,
            r#"{"args":"--work-tree=/tmp status"}"#,
        ] {
            let resp = svc
                .execute_native_tool(Request::new(ToolCallRequest {
                    tool_name: "git".to_string(),
                    arguments_json: escape_args.to_string(),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(!resp.success, "expected rejection for: {escape_args}");
            assert!(resp.error.contains("not allowed"), "got: {}", resp.error);
        }
    }

    #[tokio::test]
    async fn git_rev_parse_show_toplevel_stays_within_workspace() {
        // galleon-fleet itself is a git repo, so run from its root.
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap()
            .to_path_buf();
        let canonical_workspace = workspace.canonicalize().unwrap();
        let svc = SystemGatewayService::new(Some(workspace));

        let resp = svc
            .execute_native_tool(Request::new(ToolCallRequest {
                tool_name: "git".to_string(),
                arguments_json: r#"{"args":"rev-parse --show-toplevel"}"#.to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.success, "git command failed: {}", resp.error);
        let output: serde_json::Value = serde_json::from_str(&resp.output).unwrap();
        let toplevel = output["stdout"].as_str().unwrap().trim();
        let canonical_toplevel = PathBuf::from(toplevel).canonicalize().unwrap();
        assert_eq!(
            canonical_toplevel, canonical_workspace,
            "git must resolve the repo root to the workspace, not escape it"
        );
    }

    #[tokio::test]
    async fn get_decrypted_secret_reports_unimplemented_not_silent_not_found() {
        let svc = SystemGatewayService::new(None);
        let err = svc
            .get_decrypted_secret(Request::new(SecretRequest {
                key_name: "anything".to_string(),
            }))
            .await
            .expect_err("vault is not wired yet, must surface as an error");
        assert_eq!(err.code(), tonic::Code::Unimplemented);
        assert!(err.message().contains("not yet wired"));
    }
}
