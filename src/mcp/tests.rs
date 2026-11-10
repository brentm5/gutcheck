//! MCP tests: the server runs in-process and an rmcp client talks to it over
//! an in-memory transport, with wiremock as the API. See `docs/testing.md`.

use std::time::Duration;

use gutcheck::Settings;
use gutcheck::client::Backoff;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, RoleServer};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const FAST: Backoff = Backoff {
    initial: Duration::from_millis(1),
    max: Duration::from_millis(5),
};

/// The model from configuration in these tests.
const CONFIGURED_MODEL: &str = "configured-model";

/// A connected client, plus the server task so a test can stop it.
struct Connection {
    client: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<()>,
}

impl Connection {
    async fn start(base_url: &str) -> Self {
        let settings = Settings {
            model: CONFIGURED_MODEL.to_owned(),
            retries: 2,
            timeout: Duration::from_secs(30),
            base_url: base_url.parse().unwrap(),
        };
        let api = Client::new(&settings, "test-key")
            .unwrap()
            .with_backoff(FAST);
        let (server_side, client_side) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let running: RunningService<RoleServer, Server> =
                Server::new(api, false).serve(server_side).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client = ().serve(client_side).await.unwrap();
        Self { client, server }
    }

    async fn call(&self, arguments: Value) -> CallToolResult {
        let Value::Object(arguments) = arguments else {
            panic!("arguments must be an object");
        };
        self.client
            .peer()
            .call_tool(CallToolRequestParams::new(TOOL_NAME).with_arguments(arguments))
            .await
            .unwrap()
    }

    async fn stop(self) {
        self.client.cancel().await.unwrap();
        self.server.await.unwrap();
    }
}

/// The text of a result's single content block.
fn text(result: &CallToolResult) -> &str {
    assert_eq!(result.content.len(), 1, "{result:?}");
    result.content[0]
        .as_text()
        .expect("text content")
        .text
        .as_str()
}

fn success(result: &CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(false), "{result:?}");
    serde_json::from_str(text(result)).unwrap()
}

fn tool_error(result: &CallToolResult) -> &str {
    assert_eq!(result.is_error, Some(true), "{result:?}");
    text(result)
}

/// Mounts a `POST /v1/systemone` mock that expects `request` and answers with
/// `response`, once.
async fn expect_request(server: &MockServer, request: Value, response: Value) {
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_json(request))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}

/// A full API response around `answers`.
fn response(answers: Value) -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": answers,
        "usage": {"input_tokens": 140, "output_tokens": 14},
    })
}

fn urgent() -> Value {
    json!({"type": "noul", "instructions": "The message is urgent"})
}

#[tokio::test]
async fn lists_only_gutcheck_ask() {
    let connection = Connection::start("http://127.0.0.1:9").await;

    let tools = connection.client.peer().list_tools(None).await.unwrap();

    let names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, [TOOL_NAME]);
    let tool = &tools.tools[0];
    insta::assert_snapshot!("description", tool.description.as_deref().unwrap());
    insta::assert_json_snapshot!("input_schema", tool.input_schema);
    connection.stop().await;
}

#[tokio::test]
async fn returns_only_the_answers_for_several_questions() {
    let server = MockServer::start().await;
    let team = json!({
        "type": "choice",
        "instructions": "Which team should handle this?",
        "criteria": {
            "billing": "Payments, invoices, refunds",
            "platform": "Infra, outages, server errors",
            "other": "Anything else",
        },
    });
    let state = "Payment page throws 500 after checkout. Third time this week!";
    let answers = json!({
        "team": {
            "type": "choice",
            "choice": "platform",
            "probabilities": {"billing": 0.06, "platform": 0.91, "other": 0.03},
            "confidence": 0.91,
        },
        "urgent": {"type": "noul", "noul": 0.88},
    });
    expect_request(
        &server,
        json!({
            "model": CONFIGURED_MODEL,
            "state": state,
            "questions": {"team": team, "urgent": urgent()},
        }),
        response(answers.clone()),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": state, "questions": {"team": team, "urgent": urgent()}}))
        .await;

    assert_eq!(success(&result), answers);
    connection.stop().await;
}

#[tokio::test]
async fn object_state_and_score_questions_are_sent_as_given() {
    let server = MockServer::start().await;
    let state = json!({"subject": "Refund", "body": "Fix it."});
    let mood = json!({
        "type": "score",
        "instructions": "How frustrated is the customer?",
        "criteria": ["Calm", "Annoyed", "Angry"],
    });
    let answers = json!({
        "mood": {
            "type": "score",
            "score": 1.8,
            "legend": {"0": "Calm", "1": "Annoyed", "2": "Angry"},
            "probabilities": {"0": 0.05, "1": 0.1, "2": 0.85},
            "confidence": 0.8,
        },
    });
    expect_request(
        &server,
        json!({"model": CONFIGURED_MODEL, "state": state, "questions": {"mood": mood}}),
        response(answers.clone()),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": state, "questions": {"mood": mood}}))
        .await;

    assert_eq!(success(&result), answers);
    connection.stop().await;
}

#[rstest::rstest]
#[case::override_wins(json!("jev-other"), "jev-other")]
#[case::missing_uses_config(Value::Null, CONFIGURED_MODEL)]
#[case::blank_uses_config(json!("  "), CONFIGURED_MODEL)]
#[case::trimmed(json!(" jev-other "), "jev-other")]
#[tokio::test]
async fn model_argument_overrides_the_configured_model(
    #[case] model: Value,
    #[case] expected: &str,
) {
    let server = MockServer::start().await;
    expect_request(
        &server,
        json!({"model": expected, "state": "text", "questions": {"urgent": urgent()}}),
        response(json!({"urgent": {"type": "noul", "noul": 0.5}})),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;
    let mut arguments = json!({"state": "text", "questions": {"urgent": urgent()}});
    if !model.is_null() {
        arguments["model"] = model;
    }

    let result = connection.call(arguments).await;

    assert_eq!(
        success(&result),
        json!({"urgent": {"type": "noul", "noul": 0.5}})
    );
    connection.stop().await;
}

/// The API takes any JSON value for instructions and descriptions, and
/// `instructions` is optional, so these are sent as given.
#[tokio::test]
async fn known_types_take_any_json_for_their_text() {
    let server = MockServer::start().await;
    let questions = json!({
        "team": {
            "type": "choice",
            "instructions": {"ask": "Which team?"},
            "criteria": {"billing": {"covers": ["refunds"]}, "other": null},
        },
        "mood": {"type": "score", "criteria": ["Calm", {"level": "Angry"}]},
        "urgent": {"type": "noul", "criteria": {"true": ["outage", "data loss"]}},
    });
    let answers = json!({
        "team": {"type": "choice", "choice": "billing"},
        "mood": {"type": "score", "score": 0.2},
        "urgent": {"type": "noul", "noul": 0.1},
    });
    expect_request(
        &server,
        json!({"model": CONFIGURED_MODEL, "state": "text", "questions": questions}),
        response(answers.clone()),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": "text", "questions": questions}))
        .await;

    assert_eq!(success(&result), answers);
    connection.stop().await;
}

#[tokio::test]
async fn unknown_question_types_pass_through_unchanged() {
    let server = MockServer::start().await;
    let question = json!({
        "type": "box",
        "instructions": {"find": "the logo"},
        "future_field": [1, 2.5, null],
    });
    let answers = json!({
        "logo": {
            "type": "box",
            "box": {"top": 1, "bottom": 2, "left": 3, "right": 4},
            "confidence": 0.7,
        },
    });
    expect_request(
        &server,
        json!({"model": CONFIGURED_MODEL, "state": "text", "questions": {"logo": question}}),
        response(answers.clone()),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": "text", "questions": {"logo": question}}))
        .await;

    assert_eq!(success(&result), answers);
    connection.stop().await;
}

#[rstest::rstest]
#[case::missing_state(
    json!({"questions": {"urgent": urgent()}}),
    "state: must be a string, object or array"
)]
#[case::number_state(
    json!({"state": 1, "questions": {"urgent": urgent()}}),
    "state: must be a string, object or array"
)]
#[case::missing_questions(json!({"state": "text"}), "questions: needs at least 1 question")]
#[case::empty_questions(
    json!({"state": "text", "questions": {}}),
    "questions: needs at least 1 question"
)]
#[case::questions_not_an_object(
    json!({"state": "text", "questions": [urgent()]}),
    "questions: must be an object keyed by question ID"
)]
#[case::question_not_an_object(
    json!({"state": "text", "questions": {"urgent": "Is it?"}}),
    "questions.urgent: must be an object"
)]
#[case::empty_id(
    json!({"state": "text", "questions": {"": urgent()}}),
    "questions: question IDs must not be empty"
)]
#[case::model_not_a_string(
    json!({"model": 1, "state": "text", "questions": {"urgent": urgent()}}),
    "model: must be a string"
)]
#[case::no_choice_options(
    json!({"state": "text", "questions": {"team": {"type": "choice", "instructions": "Which?", "criteria": {}}}}),
    "questions.team: choice needs at least 1 option"
)]
#[case::too_many_score_levels(
    json!({"state": "text", "questions": {"mood": {"type": "score", "instructions": "How?", "criteria": ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]}}}),
    "questions.mood: score allows at most 10 levels"
)]
#[case::noul_criteria_with_other_keys(
    json!({"state": "text", "questions": {"urgent": {"type": "noul", "instructions": "Urgent?", "criteria": {"true": "yes", "maybe": "unsure"}}}}),
    "questions.urgent: noul criteria may only have true and false keys"
)]
#[case::choice_with_object_instructions_and_no_options(
    json!({"state": "text", "questions": {"team": {"type": "choice", "instructions": {"ask": "Which?"}, "criteria": {}}}}),
    "questions.team: choice needs at least 1 option"
)]
#[tokio::test]
async fn local_check_failures_are_tool_errors(#[case] arguments: Value, #[case] message: &str) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection.call(arguments).await;

    assert_eq!(tool_error(&result), message);
    connection.stop().await;
}

#[rstest::rstest]
#[case::validation(
    422,
    include_str!("../../tests/fixtures/validation_error_422.json"),
    "questions.noul.criteria: Field required"
)]
#[case::bad_request(
    400,
    include_str!("../../tests/fixtures/bad_request_400.json"),
    "max_tokens_exceeded"
)]
#[case::unauthorized(
    401,
    "",
    "invalid or missing API key; check it with `gutcheck auth validate`"
)]
#[case::still_unavailable(503, "", "API temporarily unavailable (HTTP 503)")]
#[tokio::test]
async fn api_errors_are_tool_errors(
    #[case] status: u16,
    #[case] body: &str,
    #[case] message: &str,
) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(&server)
        .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": "text", "questions": {"urgent": urgent()}}))
        .await;

    assert_eq!(tool_error(&result), message);
    connection.stop().await;
}

#[tokio::test]
async fn a_missing_answer_is_a_tool_error() {
    let server = MockServer::start().await;
    expect_request(
        &server,
        json!({"model": CONFIGURED_MODEL, "state": "text", "questions": {"urgent": urgent()}}),
        response(json!({"other": {"type": "noul", "noul": 0.5}})),
    )
    .await;
    let connection = Connection::start(&server.uri()).await;

    let result = connection
        .call(json!({"state": "text", "questions": {"urgent": urgent()}}))
        .await;

    assert_eq!(
        tool_error(&result),
        "unexpected API response: it has no usable answer"
    );
    connection.stop().await;
}
