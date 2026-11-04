//! Client behavior against a mock server: headers, status codes, retries and
//! the timeout. See `docs/testing.md`.

use std::time::{Duration, Instant};

use gutcheck::client::Backoff;
use gutcheck::{Client, Error, Settings};
use wiremock::matchers::{header, method, path};
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
