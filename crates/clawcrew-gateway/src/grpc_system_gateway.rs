//! `SystemGateway` gRPC server (D1): the Go engine calls into this to run
//! native-platform tools (bash, git, file I/O) that only the Rust process has
//! direct OS access to. Hosted on `:50052`, wired in [`crate::start_grpc_server`].

tonic::include_proto!("clawcrew.agent");

use system_gateway_server::SystemGateway;
use tonic::{Request, Response, Status};

/// Tools the engine is allowed to request today. Anything else is rejected
/// before it reaches real execution.
const ALLOWED_TOOLS: &[&str] = &["bash", "read_file", "write_file", "git"];

#[derive(Debug, Default)]
pub struct SystemGatewayService;

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
            }));
        }

        // ponytail: stub execution — allowlist + plumbing proven end-to-end
        // first; real bash/git/file-IO dispatch is the next ticket, not this
        // one. Ceiling: every allowed call returns the same canned success.
        Ok(Response::new(ToolCallResponse {
            success: true,
            output: format!("stub: '{}' accepted (not yet executed)", req.tool_name),
            error: String::new(),
        }))
    }

    async fn get_decrypted_secret(
        &self,
        _request: Request<SecretRequest>,
    ) -> Result<Response<SecretResponse>, Status> {
        // ponytail: stub — Secret Vault wiring is a separate ticket.
        Ok(Response::new(SecretResponse {
            found: false,
            value: String::new(),
        }))
    }

    async fn execute_task(
        &self,
        request: Request<TaskExecutionRequest>,
    ) -> Result<Response<TaskExecutionResponse>, Status> {
        // ponytail: stub — external task-executor dispatch is a separate ticket.
        let task_id = request.into_inner().task_id;
        Ok(Response::new(TaskExecutionResponse {
            success: true,
            output: format!("stub: task '{task_id}' accepted (not yet executed)"),
            error: String::new(),
        }))
    }
}
