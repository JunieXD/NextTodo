//! The MCP endpoint `/mcp`: the same tools over Streamable HTTP, without sessions. See `docs/agent.md`.

use axum::extract::Request;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use rmcp::ErrorData;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::Value;

use crate::AppState;
use crate::auth::Auth;
use crate::tools::{self, Kind};

const INSTRUCTIONS: &str = "NextTodo 是以先后关系为核心的待办：事项之间有依赖（前置完成后才能做后续），文件夹可以嵌套。\
事项用编号引用，文件夹用路径（如 /工作/网站改版）。回答“现在该做什么”用 next；\
一次建多个有先后关系的事项用 apply，先 dry_run 预览给用户看；改错了用 log 找到批次再 undo。";

struct Mcp {
    state: AppState,
}

impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nexttodo", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, ErrorData> {
        let tools = tools::list()
            .into_iter()
            .map(|t| {
                let hints = ToolAnnotations::new().read_only(t.kind == Kind::Read).destructive(t.kind == Kind::Destructive);
                Tool::new(t.name, t.description, t.input_schema).with_annotations(hints)
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        mut context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut parts = context.extensions.remove::<Parts>().expect("the HTTP transport passes the request on");
        let auth = parts.extensions.remove::<Auth>().expect("authenticate runs before every request");
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = match tools::call(&self.state, auth, &request.name, args, None).await {
            Ok(output) => CallToolResult::success(vec![ContentBlock::text(output.text)]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.describe())]),
        };
        Ok(result.into())
    }
}

pub fn service(state: AppState) -> StreamableHttpService<impl ServerHandler, NeverSessionManager> {
    // Behind a reverse proxy on whatever host it is deployed to; every request carries a token.
    let config = StreamableHttpServerConfig::default().with_legacy_session_mode(false).with_json_response(true).disable_allowed_hosts();
    StreamableHttpService::new(move || Ok(Mcp { state: state.clone() }), Default::default(), config)
}

/// Checks the token before the request reaches MCP, and hands the caller on to the tools.
pub async fn authenticate(auth: Auth, mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(auth);
    next.run(req).await
}
