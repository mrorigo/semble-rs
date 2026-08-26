use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_mcp_sdk::error::SdkResult;
use rust_mcp_sdk::macros::{JsonSchema, mcp_tool};
use rust_mcp_sdk::mcp_server::{McpServerOptions, ServerHandler, server_runtime};
use rust_mcp_sdk::schema::schema_utils::CallToolError;
use rust_mcp_sdk::schema::{
    CallToolRequestParams, CallToolResult, Implementation, InitializeResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, RpcError, ServerCapabilities, ServerCapabilitiesTools,
    TextContent,
};
use rust_mcp_sdk::{
    McpServer, StdioTransport, ToMcpServerHandler, TransportOptions, mcp_icon, tool_box,
};

use crate::index::SembleIndex;
use crate::index::dense::load_model;
use crate::types::SearchMode;
use crate::utils::{format_results, is_git_url, resolve_chunk, trace};

const CACHE_MAX_SIZE: usize = 10;

pub struct IndexCache {
    model: crate::index::dense::StaticModel,
    include_text_files: bool,
    entries: Mutex<HashMap<String, Arc<SembleIndex>>>,
    order: Mutex<VecDeque<String>>,
}

impl IndexCache {
    pub fn new(include_text_files: bool) -> Self {
        Self {
            model: load_model(None),
            include_text_files,
            entries: Mutex::new(HashMap::new()),
            order: Mutex::new(VecDeque::new()),
        }
    }

    fn cache_key(source: &str, ref_name: Option<&str>) -> String {
        if is_git_url(source) {
            match ref_name {
                Some(r) => format!("{}@{}", source, r),
                None => source.to_string(),
            }
        } else {
            std::path::Path::new(source)
                .canonicalize()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| source.to_string())
        }
    }

    pub fn get_blocking(
        &self,
        source: &str,
        ref_name: Option<&str>,
    ) -> Result<Arc<SembleIndex>, String> {
        let key = Self::cache_key(source, ref_name);
        if let Some(cached) = self
            .entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(&key).cloned())
        {
            self.touch(&key);
            return Ok(cached);
        }

        let built = if is_git_url(source) {
            Arc::new(SembleIndex::from_git(
                source,
                ref_name,
                Some(self.model.clone()),
                None,
                self.include_text_files,
            )?)
        } else {
            Arc::new(SembleIndex::from_path(
                source,
                Some(self.model.clone()),
                None,
                self.include_text_files,
            )?)
        };

        self.insert(key, built.clone());
        Ok(built)
    }

    fn insert(&self, key: String, value: Arc<SembleIndex>) {
        if let Ok(mut entries) = self.entries.lock()
            && let Ok(mut order) = self.order.lock()
        {
            if entries.len() >= CACHE_MAX_SIZE
                && let Some(oldest) = order.pop_front()
            {
                entries.remove(&oldest);
            }
            entries.insert(key.clone(), value);
            order.push_back(key);
        }
    }

    fn touch(&self, key: &str) {
        if let Ok(mut order) = self.order.lock()
            && let Some(pos) = order.iter().position(|k| k == key)
            && let Some(item) = order.remove(pos)
        {
            order.push_back(item);
        }
    }
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
#[mcp_tool(
    name = "search",
    description = "Search a codebase using semantic, BM25, or hybrid ranking.",
    title = "Semble search",
    idempotent_hint = true,
    destructive_hint = false,
    open_world_hint = true,
    read_only_hint = true
)]
pub struct SearchTool {
    pub query: String,
    pub repo: Option<String>,
    pub mode: Option<String>,
    pub top_k: Option<u32>,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
#[mcp_tool(
    name = "find_related",
    description = "Find code semantically related to a specific file path and line number.",
    title = "Semble find related",
    idempotent_hint = true,
    destructive_hint = false,
    open_world_hint = true,
    read_only_hint = true
)]
pub struct FindRelatedTool {
    pub file_path: String,
    pub line: u32,
    pub repo: Option<String>,
    pub top_k: Option<u32>,
}

tool_box!(SembleTools, [SearchTool, FindRelatedTool]);

impl SearchTool {
    fn call_tool(
        &self,
        cache: &IndexCache,
        default_source: Option<&str>,
    ) -> Result<CallToolResult, String> {
        let source = resolve_source(self.repo.as_deref(), default_source)?;
        trace(format!("MCP search resolved source={}", source));
        let index = cache.get_blocking(&source, None)?;
        let mode = match self.mode.as_deref().unwrap_or("hybrid") {
            "semantic" => SearchMode::Semantic,
            "bm25" => SearchMode::Bm25,
            _ => SearchMode::Hybrid,
        };
        let results = index.search(
            &self.query,
            self.top_k.unwrap_or(5) as usize,
            mode,
            None,
            None,
            None,
        );
        if results.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                "No results found.",
            )]));
        }
        Ok(CallToolResult::text_content(vec![TextContent::from(
            format_results(
                &format!("Search results for: {:?} (mode={:?})", self.query, mode),
                &results,
            ),
        )]))
    }
}

impl FindRelatedTool {
    fn call_tool(
        &self,
        cache: &IndexCache,
        default_source: Option<&str>,
    ) -> Result<CallToolResult, String> {
        let source = resolve_source(self.repo.as_deref(), default_source)?;
        trace(format!("MCP find_related resolved source={}", source));
        let index = cache.get_blocking(&source, None)?;
        let Some(chunk) = resolve_chunk(&index.chunks, &self.file_path, self.line as usize) else {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("No chunk found at {}:{}.", self.file_path, self.line),
            )]));
        };
        let results = index.find_related(&chunk, self.top_k.unwrap_or(5) as usize);
        if results.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!(
                    "No related chunks found for {}:{}.",
                    self.file_path, self.line
                ),
            )]));
        }
        Ok(CallToolResult::text_content(vec![TextContent::from(
            format_results(
                &format!("Chunks related to {}:{}", self.file_path, self.line),
                &results,
            ),
        )]))
    }
}

fn resolve_source(repo: Option<&str>, default_source: Option<&str>) -> Result<String, String> {
    let source = repo.or(default_source).ok_or_else(|| {
        "No repo specified and no default index. Pass an https:// or http:// git URL or local directory path as `repo`.".to_string()
    })?;
    if is_git_url(source) && !source.starts_with("https://") && !source.starts_with("http://") {
        return Err(format!(
            "Only https://, http://, or local directory paths are accepted as `repo`. Got: {:?}",
            source
        ));
    }
    Ok(source.to_string())
}

struct SembleServerHandler {
    cache: Arc<IndexCache>,
    default_source: Option<String>,
}

#[async_trait]
impl ServerHandler for SembleServerHandler {
    async fn handle_list_tools_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            meta: None,
            next_cursor: None,
            tools: vec![SearchTool::tool(), FindRelatedTool::tool()],
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let tools: SembleTools = SembleTools::try_from(params).map_err(CallToolError::new)?;
        let result = match tools {
            SembleTools::SearchTool(tool) => {
                tool.call_tool(&self.cache, self.default_source.as_deref())
            }
            SembleTools::FindRelatedTool(tool) => {
                tool.call_tool(&self.cache, self.default_source.as_deref())
            }
        };
        result.map_err(|err| CallToolError::new(std::io::Error::other(err)))
    }
}

pub async fn serve(
    path: Option<String>,
    ref_name: Option<String>,
    include_text_files: bool,
) -> SdkResult<()> {
    trace(format!(
        "starting MCP server path={:?} ref_name={:?} include_text_files={}",
        path, ref_name, include_text_files
    ));
    let server_details = InitializeResult {
        server_info: Implementation {
            name: "semble".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some("Semble".into()),
            description: Some("Native Rust semantic search for codebases.".into()),
            icons: vec![mcp_icon!(
                src = "https://raw.githubusercontent.com/rust-mcp-stack/rust-mcp-sdk/main/assets/rust-mcp-icon.png",
                mime_type = "image/png",
                sizes = ["128x128"],
                theme = "light"
            )],
            website_url: Some("https://github.com/mrorigo/semble-rs".into()),
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        instructions: Some(
            "Use search to find relevant code, then find_related to explore nearby implementations."
                .into(),
        ),
        meta: None,
    };
    let transport = StdioTransport::new(TransportOptions::default())?;
    let handler = SembleServerHandler {
        cache: Arc::new(IndexCache::new(include_text_files)),
        default_source: path,
    };
    if let Some(source) = handler.default_source.clone() {
        let _ = handler.cache.get_blocking(&source, ref_name.as_deref());
    }
    let server = server_runtime::create_server(McpServerOptions {
        server_details,
        transport,
        handler: handler.to_mcp_server_handler(),
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });
    server.start().await
}
