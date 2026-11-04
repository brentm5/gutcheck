use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gutcheck() -> Command {
    Command::cargo_bin("gutcheck").expect("binary should build")
}

/// A config file that points at `server` and doesn't retry.
fn config_for(server: &MockServer) -> NamedTempFile {
    config_with(&format!("base_url = {:?}\nretries = 0", server.uri()))
}

fn config_with(contents: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file
}

fn auth_validate(config: &NamedTempFile) -> Command {
    let mut command = gutcheck();
    command
        .args(["auth", "validate", "--config"])
        .arg(config.path())
        .env("TYPESAFE_API_KEY", "test-key");
    command
}

async fn mock_models(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

#[test]
fn version_prints_name_and_version() {
    gutcheck()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("gutcheck {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn no_arguments_prints_help() {
    gutcheck()
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Usage: gutcheck"));
}

#[test]
fn help_goes_to_stdout_and_succeeds() {
    gutcheck()
        .args(["auth", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("validate"));
}

#[test]
fn bad_flag_values_exit_1() {
    gutcheck()
        .args(["auth", "validate", "--timeout", "soon"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("invalid duration"));
}

#[tokio::test]
async fn auth_validate_succeeds_with_a_working_key() {
    let server = MockServer::start().await;
    mock_models(&server, 200, "{}").await;

    auth_validate(&config_for(&server))
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[tokio::test]
async fn global_flags_work_before_the_subcommand() {
    let server = MockServer::start().await;
    mock_models(&server, 200, "{}").await;
    let config = config_for(&server);

    gutcheck()
        .arg("--config")
        .arg(config.path())
        .args(["--timeout", "10s", "--model", "jev-1", "auth", "validate"])
        .env("TYPESAFE_API_KEY", "test-key")
        .assert()
        .success();
}

#[tokio::test]
async fn auth_validate_reports_a_rejected_key() {
    let server = MockServer::start().await;
    mock_models(&server, 401, "secret detail").await;

    auth_validate(&config_for(&server))
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: invalid or missing API key\n");
}

#[tokio::test]
async fn verbose_adds_the_status_and_body() {
    let server = MockServer::start().await;
    mock_models(&server, 401, "secret detail").await;

    auth_validate(&config_for(&server))
        .arg("--verbose")
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "error: invalid or missing API key\nHTTP status: 401\nresponse body: secret detail\n",
        );
}

#[tokio::test]
async fn auth_validate_retries_temporary_failures() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(529))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    mock_models(&server, 200, "{}").await;

    auth_validate(&config_with(&format!(
        "base_url = {:?}\nretries = 1",
        server.uri()
    )))
    .assert()
    .success();
}

#[tokio::test]
async fn timeout_flag_overrides_the_config_file() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(30)))
        .mount(&server)
        .await;

    auth_validate(&config_for(&server))
        .args(["--timeout", "100ms"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: timed out after 100ms\n");
}

#[tokio::test]
async fn missing_or_empty_key_fails_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let config = config_for(&server);

    auth_validate(&config)
        .env_remove("TYPESAFE_API_KEY")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: TYPESAFE_API_KEY is not set\n");

    auth_validate(&config)
        .env("TYPESAFE_API_KEY", "")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: TYPESAFE_API_KEY is not set\n");
}

#[test]
fn missing_config_file_fails() {
    gutcheck()
        .args(["auth", "validate", "--config", "/nonexistent/config.toml"])
        .env("TYPESAFE_API_KEY", "test-key")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: config file not found: /nonexistent/config.toml\n");
}

#[test]
fn invalid_base_url_fails() {
    auth_validate(&config_with("base_url = \"not a url\""))
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("base_url"));
}

#[cfg(unix)]
#[test]
fn non_unicode_key_is_reported_as_invalid() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    auth_validate(&config_with(""))
        .env("TYPESAFE_API_KEY", OsStr::from_bytes(b"\xff"))
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: TYPESAFE_API_KEY contains invalid characters\n");
}
