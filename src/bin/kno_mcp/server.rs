use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::{extract::Request, response::Response, Router};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{
    CallToolResult, Implementation, InitializeRequestParams, InitializeResult, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::{MaybeSendFuture, RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};

use crate::auth::bearer_token_matches;
use crate::runner::KnoRunner;
use crate::session::LeaseRegistry;
use crate::sync_loop::spawn_background_sync;
use crate::tools::{
    claim_argv, create_argv, id_argv, list_argv, update_argv, ClaimArgs, CreateArgs, IdArgs,
    ListArgs, UpdateArgs,
};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub repo: PathBuf,
    pub kno_bin: PathBuf,
    pub lease_timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub bind: String,
    pub token: String,
    pub sync_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct KnoMcp {
    runner: KnoRunner,
    lease_registry: LeaseRegistry,
    lease_timeout_seconds: u64,
    tool_router: ToolRouter<Self>,
}

impl KnoMcp {
    pub fn new(config: ServerConfig) -> Self {
        Self::with_lease_registry(config, LeaseRegistry::default())
    }

    fn with_lease_registry(config: ServerConfig, lease_registry: LeaseRegistry) -> Self {
        Self {
            runner: KnoRunner::new(config.kno_bin, config.repo),
            lease_registry,
            lease_timeout_seconds: config.lease_timeout_seconds,
            tool_router: Self::tool_router(),
        }
    }

    fn run_tool(&self, subcommand: &str, args: Vec<String>) -> CallToolResult {
        self.runner.run_tool(subcommand, &args)
    }

    fn run_mutating_tool(&self, subcommand: &str, args: Vec<String>) -> CallToolResult {
        let result = self.runner.run_tool(subcommand, &args);
        match self.runner.run_allowing_active_leases("push", &[]) {
            Ok(value) => eprintln!(
                "kno-mcp post-mutation push after {subcommand}: {}",
                push_detail(&value)
            ),
            Err(err) => eprintln!(
                "kno-mcp post-mutation push failed after {subcommand}: {}",
                err.stderr
            ),
        }
        result
    }
}

fn push_detail(value: &serde_json::Value) -> String {
    let copied = value
        .get("copied_files")
        .and_then(serde_json::Value::as_u64);
    let pushed = value.get("pushed").and_then(serde_json::Value::as_bool);
    format!("copied_files={copied:?} pushed={pushed:?}")
}

#[tool_router(router = tool_router)]
impl KnoMcp {
    #[tool(description = "List knots, optionally filtered by state, tag, type, or limit.")]
    pub async fn knots_list(&self, Parameters(args): Parameters<ListArgs>) -> CallToolResult {
        self.run_tool("ls", list_argv(args))
    }

    #[tool(description = "Show a single knot by id or alias.")]
    pub async fn knots_show(&self, Parameters(args): Parameters<IdArgs>) -> CallToolResult {
        self.run_tool("show", id_argv(args))
    }

    #[tool(description = "Poll for the highest-priority claimable knot.")]
    pub async fn knots_poll(&self) -> CallToolResult {
        self.run_tool("poll", Vec::new())
    }

    #[tool(description = "Create a new knot and return the created KnotView JSON.")]
    pub async fn knots_create(&self, Parameters(args): Parameters<CreateArgs>) -> CallToolResult {
        let priority = args.priority;
        let result = self.run_mutating_tool("new", create_argv(args));
        if let (Some(priority), Some(id)) = (priority, created_id(&result)) {
            let update_args = vec![id, "--priority".to_string(), priority.to_string()];
            return self.run_mutating_tool("update", update_args);
        }
        result
    }

    #[tool(description = "Update knot fields.")]
    pub async fn knots_update(&self, Parameters(args): Parameters<UpdateArgs>) -> CallToolResult {
        self.run_mutating_tool("update", update_argv(args))
    }

    #[tool(description = "Claim a knot, preserving the CLI claim-boundary contract.")]
    pub async fn knots_claim(&self, Parameters(args): Parameters<ClaimArgs>) -> CallToolResult {
        self.run_mutating_tool("claim", claim_argv(args, self.lease_registry.current()))
    }

    #[tool(description = "Advance a claimed knot to its next workflow state.")]
    pub async fn knots_next(&self, Parameters(args): Parameters<IdArgs>) -> CallToolResult {
        let mut argv = id_argv(args);
        if let Some(lease) = self.lease_registry.current() {
            argv.push("--lease".to_string());
            argv.push(lease);
        }
        self.run_mutating_tool("next", argv)
    }

    #[tool(description = "Roll back a knot from an action state to its prior ready state.")]
    pub async fn knots_rollback(&self, Parameters(args): Parameters<IdArgs>) -> CallToolResult {
        self.run_mutating_tool("rollback", id_argv(args))
    }

    #[tool(description = "Run Knots git sync and return the SyncOutcome JSON.")]
    pub async fn knots_sync(&self) -> CallToolResult {
        self.run_tool("sync", Vec::new())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for KnoMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("kno-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Knots MCP server")
    }

    #[cfg(not(tarpaulin_include))]
    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, McpError>> + MaybeSendFuture + '_ {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request.clone());
        }
        async move {
            self.lease_registry
                .get_or_create(
                    &self.runner,
                    &request.client_info,
                    self.lease_timeout_seconds,
                )
                .map_err(|err| McpError::internal_error(err, None))?;
            Ok(self.get_info())
        }
    }
}

#[cfg(not(tarpaulin_include))]
pub async fn serve_http(
    server_config: ServerConfig,
    http: HttpConfig,
) -> Result<(), Box<dyn Error>> {
    let runner = KnoRunner::new(server_config.kno_bin.clone(), server_config.repo.clone());
    spawn_background_sync(runner, http.sync_interval);
    let token = http.token.clone();
    let lease_registry = LeaseRegistry::default();
    let service: StreamableHttpService<KnoMcp, LocalSessionManager> = StreamableHttpService::new(
        move || {
            Ok(KnoMcp::with_lease_registry(
                server_config.clone(),
                lease_registry.clone(),
            ))
        },
        Default::default(),
        StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .disable_allowed_hosts(),
    );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(move |request, next| {
            auth_middleware(request, next, token.clone())
        }));
    let listener = tokio::net::TcpListener::bind(&http.bind).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(not(tarpaulin_include))]
async fn auth_middleware(
    mut request: Request,
    next: Next,
    token: String,
) -> Result<Response, StatusCode> {
    if authorized(request.headers(), &token) {
        ensure_mcp_accept_header(&mut request);
        ensure_mcp_content_type_header(&mut request);
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| bearer_token_matches(Some(value), token))
}

fn ensure_mcp_accept_header(request: &mut Request) {
    let accept_is_missing_or_wildcard = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim() == "*/*")
        .unwrap_or(true);
    if accept_is_missing_or_wildcard {
        request.headers_mut().insert(
            header::ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
    }
}

fn ensure_mcp_content_type_header(request: &mut Request) {
    let content_type_is_missing_or_curl_default = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.starts_with("application/x-www-form-urlencoded"))
        .unwrap_or(true);
    if content_type_is_missing_or_curl_default {
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
}

fn created_id(result: &CallToolResult) -> Option<String> {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::HeaderValue;
    use rmcp::ServerHandler;

    #[test]
    fn tool_router_lists_core_tools() {
        let server = KnoMcp::new(ServerConfig {
            repo: PathBuf::from("/example/repo"),
            kno_bin: PathBuf::from("/example/kno"),
            lease_timeout_seconds: 600,
        });
        let tools = server.tool_router.list_all();
        let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert!(names.contains(&"knots_list"));
        assert!(names.contains(&"knots_claim"));
        assert!(names.contains(&"knots_next"));
        assert!(names.contains(&"knots_sync"));
        assert!(names.len() >= 7);
    }

    #[test]
    fn authorization_checks_bearer_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        assert!(authorized(&headers, "secret"));
        assert!(!authorized(&headers, "wrong"));
    }

    #[test]
    fn accept_header_is_supplied_when_curl_omits_it() {
        let mut request = Request::builder().body(Body::empty()).expect("request");
        ensure_mcp_accept_header(&mut request);
        assert_eq!(
            request.headers().get(header::ACCEPT),
            Some(&HeaderValue::from_static(
                "application/json, text/event-stream"
            ))
        );

        request
            .headers_mut()
            .insert(header::ACCEPT, HeaderValue::from_static("*/*"));
        ensure_mcp_accept_header(&mut request);
        assert_eq!(
            request.headers().get(header::ACCEPT),
            Some(&HeaderValue::from_static(
                "application/json, text/event-stream"
            ))
        );
    }

    #[test]
    fn content_type_is_supplied_when_curl_uses_form_default() {
        let mut request = Request::builder().body(Body::empty()).expect("request");
        ensure_mcp_content_type_header(&mut request);
        assert_eq!(
            request.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );

        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        ensure_mcp_content_type_header(&mut request);
        assert_eq!(
            request.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
    }

    #[test]
    fn created_id_reads_structured_content() {
        let result = CallToolResult::structured(serde_json::json!({ "id": "k1" }));
        assert_eq!(created_id(&result), Some("k1".to_string()));

        let result = CallToolResult::structured(serde_json::json!({ "data": [] }));
        assert_eq!(created_id(&result), None);
    }

    #[tokio::test]
    async fn server_tools_route_to_kno_runner() {
        let server = server_with_fixture();
        let mut client = Implementation::new("client", "1.0");
        client.title = Some("provider".to_string());
        server
            .lease_registry
            .get_or_create(&server.runner, &client, 60)
            .expect("lease");

        let list = server
            .knots_list(Parameters(ListArgs {
                state: Some("ready".to_string()),
                tag: None,
                knot_type: None,
                limit: Some(1),
                offset: Some(0),
            }))
            .await;
        assert_eq!(list.structured_content.unwrap()["total"], 1);

        assert_eq!(
            server
                .knots_show(Parameters(IdArgs {
                    id: "k1".to_string()
                }))
                .await
                .structured_content
                .unwrap()["id"],
            "k1"
        );
        assert_eq!(
            server.knots_poll().await.structured_content.unwrap()["id"],
            "k1"
        );
        assert_eq!(
            server
                .knots_create(Parameters(CreateArgs {
                    title: "New".to_string(),
                    description: Some("Desc".to_string()),
                    acceptance: Some("Done".to_string()),
                    knot_type: Some("work".to_string()),
                    priority: Some(3),
                }))
                .await
                .structured_content
                .unwrap()["priority"],
            3
        );
        assert_eq!(
            server
                .knots_update(Parameters(UpdateArgs {
                    id: "k1".to_string(),
                    title: Some("Updated".to_string()),
                    description: None,
                    acceptance: None,
                    priority: None,
                    state: None,
                }))
                .await
                .structured_content
                .unwrap()["title"],
            "updated"
        );
        assert_eq!(
            server
                .knots_claim(Parameters(ClaimArgs {
                    id: "k1".to_string(),
                    e2e: Some(false),
                }))
                .await
                .structured_content
                .unwrap()["workflow_boundary_kind"],
            "single_action"
        );
        assert_eq!(
            server
                .knots_next(Parameters(IdArgs {
                    id: "k1".to_string()
                }))
                .await
                .structured_content
                .unwrap()["state"],
            "ready_for_review"
        );
        assert_eq!(
            server
                .knots_rollback(Parameters(IdArgs {
                    id: "k1".to_string()
                }))
                .await
                .structured_content
                .unwrap()["target_state"],
            "ready_for_implementation"
        );
        assert_eq!(
            server.knots_sync().await.structured_content.unwrap()["status"],
            "deferred"
        );
    }

    #[test]
    fn server_info_advertises_tools() {
        let info = server_with_fixture().get_info();
        assert_eq!(info.server_info.name, "kno-mcp");
        assert!(info.capabilities.tools.is_some());
    }

    #[test]
    fn shared_registry_carries_lease_between_http_instances() {
        let registry = LeaseRegistry::default();
        let first = server_with_registry(registry.clone());
        let second = server_with_registry(registry);
        let mut client = Implementation::new("client", "1.0");
        client.title = Some("provider".to_string());

        first
            .lease_registry
            .get_or_create(&first.runner, &client, 60)
            .expect("lease");

        assert_eq!(second.lease_registry.current(), Some("L1".to_string()));
    }

    fn server_with_fixture() -> KnoMcp {
        server_with_registry(LeaseRegistry::default())
    }

    fn server_with_registry(lease_registry: LeaseRegistry) -> KnoMcp {
        KnoMcp::with_lease_registry(
            ServerConfig {
                repo: PathBuf::from("/example/repo"),
                kno_bin: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/kno-stub.sh"),
                lease_timeout_seconds: 600,
            },
            lease_registry,
        )
    }
}
