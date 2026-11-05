//! Client behavior against a mock server: headers, status codes, retries and
//! the timeout. See `docs/testing.md`.

use std::time::{Duration, Instant};

use gutcheck::client::Backoff;
use gutcheck::{Client, Error, Question, Settings};
use rstest::rstest;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FAST: Backoff = Backoff {
    initial: Duration::from_millis(1),
    max: Duration::from_millis(5),
};

fn client(base_url: &str, retries: u32, timeout: Duration) -> Client {
    let settings = Settings {
        model: "jev-latest".to_owned(),
        retries,
        timeout,
        base_url: base_url.parse().unwrap(),
    };
    Client::new(&settings, "test-key")
        .unwrap()
        .with_backoff(FAST)
}

async fn respond(server: &MockServer, status: u16, expected_requests: u64) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(status))
        .expect(expected_requests)
        .mount(server)
        .await;
}

const LONG: Duration = Duration::from_secs(30);

#[tokio::test]
async fn sends_the_key_as_a_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client(&server.uri(), 0, LONG)
        .validate_auth()
        .await
        .unwrap();
}

#[tokio::test]
async fn keeps_a_path_prefix_and_ignores_a_trailing_slash() {
    let server = MockServer::start().await;
    Mock::given(path("/typesafe/v1/models"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let base_url = format!("{}/typesafe/", server.uri());
    client(&base_url, 0, LONG).validate_auth().await.unwrap();
}

#[tokio::test]
async fn unauthorized_is_rejected_without_retrying() {
    let server = MockServer::start().await;
    respond(&server, 401, 1).await;

    let error = client(&server.uri(), 5, LONG)
        .validate_auth()
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Rejected { .. }));
    assert_eq!(error.to_string(), "invalid or missing API key");
}

#[tokio::test]
async fn other_client_errors_are_not_retried() {
    let server = MockServer::start().await;
    respond(&server, 403, 1).await;

    let error = client(&server.uri(), 5, LONG)
        .validate_auth()
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Rejected { .. }));
    assert_eq!(error.to_string(), "unexpected API response (HTTP 403)");
}

#[tokio::test]
async fn redirects_are_rejected_and_not_followed() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/login"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let error = client(&server.uri(), 5, LONG)
        .validate_auth()
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Rejected { .. }));
    assert_eq!(error.to_string(), "unexpected API response (HTTP 302)");
}

#[tokio::test]
async fn temporary_statuses_are_retried_up_to_the_limit() {
    for status in [408, 429, 500, 503, 529] {
        let server = MockServer::start().await;
        respond(&server, status, 3).await;

        let error = client(&server.uri(), 2, LONG)
            .validate_auth()
            .await
            .unwrap_err();

        assert!(
            matches!(error, Error::Temporary { .. }),
            "{status}: {error}"
        );
        assert_eq!(error.http_details().unwrap().status, status);
    }
}

#[tokio::test]
async fn zero_retries_means_one_attempt() {
    let server = MockServer::start().await;
    respond(&server, 503, 1).await;

    client(&server.uri(), 0, LONG)
        .validate_auth()
        .await
        .unwrap_err();
}

#[tokio::test]
async fn succeeds_when_a_retry_works() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(529))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    respond(&server, 200, 1).await;

    client(&server.uri(), 2, LONG)
        .validate_auth()
        .await
        .unwrap();
}

#[tokio::test]
async fn network_errors_are_retried_then_reported() {
    // Bind a free port, then close it so connections are refused.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let error = client(&uri, 1, LONG).validate_auth().await.unwrap_err();

    assert!(matches!(error, Error::Temporary { details: None, .. }));
    let message = error.to_string();
    assert!(message.starts_with("network error: "), "{message}");
    assert!(!message.contains(&uri), "{message}");
    assert!(!message.contains('\n'), "{message}");
}

#[tokio::test]
async fn retry_after_is_honored() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after-ms", "200"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    respond(&server, 200, 1).await;

    let start = Instant::now();
    client(&server.uri(), 1, LONG)
        .validate_auth()
        .await
        .unwrap();

    assert!(start.elapsed() >= Duration::from_millis(200));
}

#[tokio::test]
async fn stops_early_when_the_next_wait_would_pass_the_timeout() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "60"))
        .expect(1)
        .mount(&server)
        .await;

    let start = Instant::now();
    let error = client(&server.uri(), 100, Duration::from_secs(5))
        .validate_auth()
        .await
        .unwrap_err();

    // It reports the last error straight away rather than a timeout.
    assert!(matches!(error, Error::Temporary { .. }), "{error}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn timeout_cancels_a_slow_response() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;

    let start = Instant::now();
    let error = client(&server.uri(), 0, Duration::from_millis(100))
        .validate_auth()
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Timeout(_)), "{error}");
    assert_eq!(error.to_string(), "timed out after 100ms");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn invalid_key_characters_are_reported_without_the_key() {
    let settings = Settings {
        model: "jev-latest".to_owned(),
        retries: 0,
        timeout: LONG,
        base_url: "http://localhost".parse().unwrap(),
    };

    let Err(error) = Client::new(&settings, "secret\nkey") else {
        panic!("a key with a newline should be rejected");
    };

    assert_eq!(
        error.to_string(),
        "TYPESAFE_API_KEY contains invalid characters"
    );
}

fn noul() -> Question {
    Question::noul("Is it urgent?".to_owned(), Some("Yes".to_owned()), None)
}

async fn mock_systemone(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

const NOUL_SUCCESS: &str = include_str!("fixtures/noul_success.json");

#[tokio::test]
async fn ask_sends_the_documented_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(header("authorization", "Bearer test-key"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "model": "jev-latest",
            "state": "the input",
            "questions": {
                "noul": {
                    "type": "noul",
                    "instructions": "Is it urgent?",
                    "criteria": {"true": "Yes"}
                }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(NOUL_SUCCESS))
        .expect(1)
        .mount(&server)
        .await;

    let answer = client(&server.uri(), 0, LONG)
        .ask_question(json!("the input"), noul())
        .await
        .unwrap();

    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"type":"noul","noul":0.91}"#
    );
}

#[tokio::test]
async fn ask_keeps_the_answer_as_the_api_wrote_it() {
    let server = MockServer::start().await;
    mock_systemone(
        &server,
        200,
        r#"{"answers":{"noul":{"type":"noul","noul":1,"confidence":0.5}}}"#,
    )
    .await;

    let answer = client(&server.uri(), 0, LONG)
        .ask_question(json!("text"), noul())
        .await
        .unwrap();

    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"type":"noul","noul":1,"confidence":0.5}"#
    );
}

#[tokio::test]
async fn ask_runs_local_checks_before_sending() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let error = client(&server.uri(), 0, LONG)
        .ask_question(json!(""), noul())
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Usage(_)), "{error}");
    assert_eq!(error.to_string(), "state: must not be empty");
}

#[tokio::test]
async fn ask_rejects_an_oversized_body_before_sending() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let error = client(&server.uri(), 0, LONG)
        .ask_question(json!("x".repeat(32 * 1024 * 1024)), noul())
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "request body is over the API's 32 MiB limit"
    );
}

#[rstest]
#[case::bad_request(400, include_str!("fixtures/bad_request_400.json"), "max_tokens_exceeded")]
#[case::unauthorized(
    401,
    "",
    "invalid or missing API key; check it with `gutcheck auth validate`"
)]
#[case::too_large(413, "", "request body is over the API's 32 MiB limit")]
#[case::encoding(415, "", "internal error: the API rejected the request encoding")]
#[case::validation(
    422,
    include_str!("fixtures/validation_error_422.json"),
    "questions.noul.criteria: Field required"
)]
#[tokio::test]
async fn ask_maps_rejections_without_retrying(
    #[case] status: u16,
    #[case] body: &str,
    #[case] message: &str,
) {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    let error = client(&server.uri(), 5, LONG)
        .ask_question(json!("text"), noul())
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Rejected { .. }), "{error}");
    assert_eq!(error.to_string(), message);
    assert_eq!(error.http_details().unwrap().status, status);
}

#[tokio::test]
async fn ask_retries_temporary_failures() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(529))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    mock_systemone(&server, 200, NOUL_SUCCESS).await;

    client(&server.uri(), 1, LONG)
        .ask_question(json!("text"), noul())
        .await
        .unwrap();
}

#[rstest]
#[case::not_json("<html>login</html>")]
#[case::no_answers("{}")]
#[case::other_id(r#"{"answers":{"other":{"type":"noul","noul":0.5}}}"#)]
#[case::wrong_type(r#"{"answers":{"noul":{"type":"box","box":{}}}}"#)]
#[case::missing_value(r#"{"answers":{"noul":{"type":"noul"}}}"#)]
#[tokio::test]
async fn ask_reports_an_unusable_success_response(#[case] body: &str) {
    let server = MockServer::start().await;
    mock_systemone(&server, 200, body).await;

    let error = client(&server.uri(), 3, LONG)
        .ask_question(json!("text"), noul())
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Rejected { .. }), "{error}");
    assert_eq!(
        error.to_string(),
        "unexpected API response: it has no usable answer"
    );
    assert_eq!(error.http_details().unwrap().body, body);
}
