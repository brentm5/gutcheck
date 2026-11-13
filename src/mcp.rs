//! `gutcheck mcp serve`: a stdio MCP server with one tool, `gutcheck_ask`.
//! See `docs/mcp-server.md`.

use std::collections::BTreeMap;
use std::sync::Arc;

use gutcheck::api::AskRequest;
use gutcheck::{Client, Error};
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value};

/// The one tool's name.
pub(crate) const TOOL_NAME: &str = "gutcheck_ask";

/// Everything an agent learns about the tool, with the input schema. The tips
/// match `docs/agent-skill.md`.
const TOOL_DESCRIPTION: &str = r#"Ask Jev, a fast and cheap model that answers structured questions about some input
("state"). Every possible answer must be defined up front, so use it for
classification, routing, rating and yes/no judgments, not for open-ended text.

Question types (set "type" on each question):
- choice: pick one option. criteria = {"option_key": "when this option applies", ...}
  (1-255 options). Returns choice, probabilities, confidence.
- score: place the state on an ordered rubric. criteria = ["lowest level", ..., "highest level"]
  (1-10 levels). Returns score (can fall between levels), legend, probabilities, confidence.
- noul: probability (0-1) that a statement is true. criteria is optional:
  {"true": "what counts as yes", "false": "what counts as no"}. Returns noul.

Tips:
- Ask several questions in one call; they run in parallel against the same state.
- Keep each question about one thing. Combine results yourself.
- Add an "other" option to choice questions when the list may not cover every input.

Returns the answers keyed by your question IDs. Errors name the invalid field so
you can fix the request and retry."#;

// The tool's input, for its schema only. Arguments are read with
// `AskRequest::from_json`, so a wrong shape becomes a tool error naming the
// field, and unknown question types pass through. Doc comments here become the
// schema's descriptions, which agents read, so this is a plain comment.
#[derive(JsonSchema)]
#[allow(dead_code)]
struct AskInput {
    /// The input the questions are about: text, or a JSON object or array
    state: Json,
    /// Questions keyed by an ID you choose; answers come back under the same IDs
    questions: BTreeMap<String, QuestionInput>,
    /// Optional. Overrides the configured model for this call
    model: Option<String>,
}

#[derive(JsonSchema)]
#[schemars(inline, untagged)]
#[allow(dead_code)]
enum Json {
    Text(String),
    Object(Map<String, Value>),
    Array(Vec<Value>),
}

#[derive(JsonSchema)]
#[schemars(inline)]
#[allow(dead_code)]
struct QuestionInput {
    /// `choice`, `score` or `noul`
    r#type: QuestionType,
    /// The question or statement to evaluate
    instructions: Option<Json>,
    /// Options (choice), ordered levels (score), or optional true/false meanings (noul)
    criteria: Option<Value>,
}

#[derive(JsonSchema)]
#[schemars(inline, rename_all = "lowercase")]
#[allow(dead_code)]
enum QuestionType {
    Choice,
    Score,
    Noul,
}

/// The `gutcheck_ask` tool definition, as `tools/list` returns it.
pub(crate) fn tool() -> Tool {
    Tool::new(TOOL_NAME, TOOL_DESCRIPTION, schema_for_type::<AskInput>())
}

/// The server. Each tool call is one operation with its own timeout.
pub(crate) struct Server {
    client: Arc<Client>,
    verbose: bool,
}

impl Server {
    pub(crate) fn new(client: Client, verbose: bool) -> Self {
        Self {
            client: Arc::new(client),
            verbose,
        }
    }

    /// Runs one `gutcheck_ask` call: the answers as JSON, or a tool error.
    async fn ask(&self, arguments: Option<Map<String, Value>>) -> CallToolResult {
        match self.answers(arguments.unwrap_or_default()).await {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(error) => {
                if self.verbose
                    && let Some(details) = error.http_details()
                {
                    eprintln!("error: {error}");
                    eprintln!("HTTP status: {}", details.status);
                    eprintln!("response body: {}", details.body);
                }
                CallToolResult::error(vec![ContentBlock::text(error.to_string())])
            }
        }
    }

    async fn answers(&self, arguments: Map<String, Value>) -> Result<String, Error> {
        let deadline = self.client.deadline();
        let request = AskRequest::from_json(arguments)?;
        let answers = self.client.ask(request, deadline).await?;
        serde_json::to_string(&answers)
            .map_err(|e| Error::Output(format!("could not encode the answers: {e}")))
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gutcheck", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![tool()],
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != TOOL_NAME {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            ));
        }
        Ok(self.ask(request.arguments).await.into())
    }
}

/// Serves MCP over stdin and stdout until the client disconnects.
pub(crate) async fn serve(server: Server) -> Result<(), Error> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| Error::Internal(format!("could not start the MCP server: {e}")))?;
    running
        .waiting()
        .await
        .map_err(|e| Error::Internal(format!("the MCP server stopped: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests;
