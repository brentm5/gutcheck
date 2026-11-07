use std::fs::File;
use std::io::{Read, Write};
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use rstest::rstest;
use serde_json::json;
use tempfile::NamedTempFile;
use wiremock::matchers::{body_partial_json, header, method, path};
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

const NOUL_SUCCESS: &str = include_str!("fixtures/noul_success.json");
const NOUL_OUTPUT: &str = "0.91\n";

fn ask_noul(config: &NamedTempFile) -> Command {
    let mut command = gutcheck();
    command
        .args(["ask", "noul", "Is it urgent?", "--config"])
        .arg(config.path())
        .env("TYPESAFE_API_KEY", "test-key");
    command
}

/// Answers only a request whose body contains `expected`.
async fn mock_noul(server: &MockServer, expected: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_partial_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_string(NOUL_SUCCESS))
        .expect(1)
        .mount(server)
        .await;
}

/// Fails the test if any request reaches the API.
async fn expect_no_requests(server: &MockServer) {
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(server)
        .await;
}

#[tokio::test]
async fn noul_prints_the_answer_for_state_from_the_flag() {
    let server = MockServer::start().await;
    mock_noul(
        &server,
        json!({
            "model": "jev-latest",
            "state": "the input",
            "questions": {"noul": {"type": "noul", "instructions": "Is it urgent?"}}
        }),
    )
    .await;

    ask_noul(&config_for(&server))
        .args(["--state", "the input"])
        .assert()
        .success()
        .stdout(NOUL_OUTPUT)
        .stderr("");
}

#[tokio::test]
async fn noul_reads_state_from_stdin() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "piped input\n"})).await;

    ask_noul(&config_for(&server))
        .write_stdin("piped input\n")
        .assert()
        .success()
        .stdout(NOUL_OUTPUT);
}

#[tokio::test]
async fn noul_sends_criteria_and_model() {
    let server = MockServer::start().await;
    mock_noul(
        &server,
        json!({
            "model": "jev-1",
            "questions": {"noul": {"criteria": {"true": "Yes", "false": "No"}}}
        }),
    )
    .await;

    ask_noul(&config_for(&server))
        .args(["--state", "text", "--true", "Yes", "--false", "No"])
        .args(["--model", "jev-1"])
        .assert()
        .success();
}

#[tokio::test]
async fn noul_state_flag_takes_precedence_over_stdin() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "text"})).await;

    ask_noul(&config_for(&server))
        .args(["--state", "text"])
        .write_stdin("piped input")
        .assert()
        .success()
        .stdout(NOUL_OUTPUT)
        .stderr("");
}

#[tokio::test]
async fn noul_rejects_missing_state() {
    let server = MockServer::start().await;
    expect_no_requests(&server).await;

    ask_noul(&config_for(&server))
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: no state given: pass --state or pipe it to stdin\n");
}

#[tokio::test]
async fn noul_leaves_an_empty_state_flag_to_the_api() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": ""})).await;

    ask_noul(&config_for(&server))
        .args(["--state", ""])
        .write_stdin("")
        .assert()
        .success()
        .stdout(NOUL_OUTPUT);
}

#[tokio::test]
async fn noul_rejects_stdin_that_is_not_utf8() {
    let server = MockServer::start().await;
    expect_no_requests(&server).await;

    ask_noul(&config_for(&server))
        .write_stdin(vec![0xff, 0xfe])
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: stdin is not valid UTF-8\n");
}

/// Runs `gutcheck ask noul` with a stdin pipe that stays open and silent, as a
/// parent process might leave it.
fn noul_with_silent_stdin(config: &NamedTempFile, args: &[&str]) -> std::process::Output {
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("gutcheck"))
        .args(["ask", "noul", "Is it urgent?", "--config"])
        .arg(config.path())
        .args(args)
        .env("TYPESAFE_API_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    output
}

#[tokio::test]
async fn state_flag_works_when_stdin_stays_open_and_silent() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "text"})).await;

    let output = noul_with_silent_stdin(&config_for(&server), &["--state", "text"]);

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), NOUL_OUTPUT);
}

fn noul_with_stdin(config: &NamedTempFile, args: &[&str], stdin: impl Into<Stdio>) -> StdCommand {
    let mut command = StdCommand::new(assert_cmd::cargo::cargo_bin("gutcheck"));
    command
        .args(["ask", "noul", "Is it urgent?", "--config"])
        .arg(config.path())
        .args(args)
        .env("TYPESAFE_API_KEY", "test-key")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[tokio::test]
async fn state_flag_works_when_stdin_has_nothing_to_read() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_string(NOUL_SUCCESS))
        .expect(4)
        .mount(&server)
        .await;
    let config = config_for(&server);

    let (closed_pipe, writer) = std::io::pipe().unwrap();
    drop(writer);
    let empty_file = NamedTempFile::new().unwrap();
    let mut file_at_its_end = NamedTempFile::new().unwrap();
    file_at_its_end.write_all(b"used up").unwrap();
    let mut used_up = File::open(file_at_its_end.path()).unwrap();
    used_up.read_to_end(&mut Vec::new()).unwrap();

    let stdins: [Stdio; 4] = [
        Stdio::null(),
        closed_pipe.into(),
        File::open(empty_file.path()).unwrap().into(),
        used_up.into(),
    ];
    for stdin in stdins {
        let output = noul_with_stdin(&config, &["--state", "text"], stdin)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), NOUL_OUTPUT);
    }
}

/// The caller may still want what's on stdin, as in a `while read` loop, so a
/// command mustn't use any of it up.
#[tokio::test]
async fn state_flag_leaves_a_file_on_stdin_unread() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "text"})).await;
    let mut input = NamedTempFile::new().unwrap();
    input.write_all(b"line one\n").unwrap();
    let mut reader = File::open(input.path()).unwrap();

    let output = noul_with_stdin(
        &config_for(&server),
        &["--state", "text"],
        reader.try_clone().unwrap(),
    )
    .output()
    .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), NOUL_OUTPUT);
    assert!(output.stderr.is_empty());
    let mut left = String::new();
    reader.read_to_string(&mut left).unwrap();
    assert_eq!(left, "line one\n");
}

#[tokio::test]
async fn state_flag_leaves_late_stdin_data_for_the_caller() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(NOUL_SUCCESS)
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    let config = config_for(&server);
    let (mut reader, mut writer) = std::io::pipe().unwrap();

    let child = noul_with_stdin(&config, &["--state", "text"], reader.try_clone().unwrap())
        .spawn()
        .unwrap();
    // A request means input selection is over; later stdin data must remain unread.
    tokio::time::timeout(Duration::from_secs(20), async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the request should arrive");
    writer.write_all(b"x").unwrap();
    drop(writer);
    let output = child.wait_with_output().unwrap();

    assert_eq!(String::from_utf8(output.stdout).unwrap(), NOUL_OUTPUT);
    let mut left = Vec::new();
    reader.read_to_end(&mut left).unwrap();
    assert_eq!(left, b"x");
}

#[tokio::test]
async fn noul_stops_reading_stdin_past_the_size_limit() {
    let server = MockServer::start().await;
    expect_no_requests(&server).await;
    let mut endless = StdCommand::new("cat")
        .arg("/dev/zero")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let output = noul_with_stdin(&config_for(&server), &[], endless.stdout.take().unwrap())
        .output()
        .unwrap();
    endless.kill().unwrap();
    endless.wait().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: request body is over the API's 32 MiB limit\n"
    );
}

#[tokio::test]
async fn timeout_too_long_for_the_clock_still_works() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "text"})).await;

    ask_noul(&config_for(&server))
        .args(["--state", "text", "--timeout", "18446744073709551615s"])
        .write_stdin("")
        .assert()
        .success()
        .stdout(NOUL_OUTPUT);
}

#[tokio::test]
async fn timeout_covers_waiting_on_stdin() {
    let server = MockServer::start().await;
    expect_no_requests(&server).await;

    let output = noul_with_silent_stdin(&config_for(&server), &["--timeout", "100ms"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: timed out after 100ms\n"
    );
}

#[tokio::test]
async fn noul_reports_api_errors_with_verbose_details() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/validation_error_422.json");
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(422).set_body_string(body))
        .mount(&server)
        .await;

    ask_noul(&config_for(&server))
        .args(["--state", "text", "--verbose"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(format!(
            "error: questions.noul.criteria: Field required\nHTTP status: 422\nresponse body: {body}\n"
        ));
}

#[test]
fn noul_needs_the_api_key_before_reading_state() {
    let config = config_with("");

    ask_noul(&config)
        .env_remove("TYPESAFE_API_KEY")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: TYPESAFE_API_KEY is not set\n");
}

async fn mock_any_noul(server: &MockServer) {
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_string(NOUL_SUCCESS))
        .mount(server)
        .await;
}

// The mock answers with a probability of 0.91.
#[rstest]
#[case::probability(&[], "0.91\n")]
#[case::default_boolean(&["--boolean"], "true\n")]
#[case::below_the_answer(&["--boolean", "--threshold", "0.9"], "true\n")]
#[case::equal_to_the_answer(&["--boolean", "--threshold", "0.91"], "false\n")]
#[case::above_the_answer(&["--boolean", "--threshold", "0.95"], "false\n")]
#[case::zero_cutoff(&["--boolean", "--threshold", "0"], "true\n")]
#[case::one_cutoff(&["--boolean", "--threshold", "1"], "false\n")]
#[case::json(&["--json"], "{\"type\":\"noul\",\"noul\":0.91}\n")]
#[tokio::test]
async fn noul_output_follows_the_flags(#[case] flags: &[&str], #[case] expected: &str) {
    let server = MockServer::start().await;
    mock_any_noul(&server).await;

    ask_noul(&config_for(&server))
        .args(["--state", "text"])
        .args(flags)
        .assert()
        .success()
        .stdout(expected.to_owned())
        .stderr("");
}

#[tokio::test]
async fn noul_false_answer_still_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/systemone"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"answers":{"noul":{"type":"noul","noul":0}}}"#),
        )
        .mount(&server)
        .await;

    ask_noul(&config_for(&server))
        .args(["--state", "text", "--boolean"])
        .assert()
        .code(0)
        .stdout("false\n");
}

#[rstest]
#[case::above_range("1.5")]
#[case::below_range("-0.1")]
#[case::not_a_number("NaN")]
#[case::text("high")]
fn invalid_thresholds_exit_1(#[case] threshold: &str) {
    ask_noul(&config_with(""))
        .env_remove("TYPESAFE_API_KEY")
        .args(["--state", "text", "--boolean"])
        .arg(format!("--threshold={threshold}"))
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("must be a number from 0 to 1"));
}

#[test]
fn boolean_and_json_cannot_be_combined() {
    ask_noul(&config_with(""))
        .env_remove("TYPESAFE_API_KEY")
        .args(["--state", "text", "--boolean", "--json"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn threshold_requires_boolean() {
    ask_noul(&config_with(""))
        .env_remove("TYPESAFE_API_KEY")
        .args(["--state", "text", "--threshold", "0.7"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("--boolean"));
}

#[rstest]
#[case::state(&["--state", "first", "--state", "second"])]
#[case::boolean(&["--boolean", "--boolean"])]
#[case::threshold(&["--boolean", "--threshold", "0.5", "--threshold", "0.7"])]
#[case::global(&["--verbose", "--verbose"])]
fn repeated_single_value_flags_are_rejected(#[case] flags: &[&str]) {
    ask_noul(&config_with(""))
        .env_remove("TYPESAFE_API_KEY")
        .args(flags)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("multiple times"));
}

#[rstest]
#[case::round_down("0.912345678", &[], "0.912\n")]
#[case::round_up("0.9126", &[], "0.913\n")]
#[case::integer("1.0", &[], "1\n")]
#[case::trim_zeros("0.0100", &[], "0.01\n")]
#[case::tiny("1e-8", &[], "0\n")]
#[case::near_cutoff("0.5004", &[], "0.5\n")]
#[case::compare_before_rounding("0.5004", &["--boolean"], "true\n")]
#[case::json_preserves_precision("0.5004", &["--json"], "{\"type\":\"noul\",\"noul\":0.5004}\n")]
#[case::exact_middle("0.5", &["--boolean"], "false\n")]
#[tokio::test]
async fn noul_rounding_only_affects_plain_numeric_output(
    #[case] number: &str,
    #[case] flags: &[&str],
    #[case] expected: &str,
) {
    let server = MockServer::start().await;
    let body = format!(r#"{{"answers":{{"noul":{{"type":"noul","noul":{number}}}}}}}"#);
    Mock::given(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    ask_noul(&config_for(&server))
        .args(["--state", "text"])
        .args(flags)
        .assert()
        .success()
        .stdout(expected.to_owned());
}

#[tokio::test]
async fn state_flag_works_in_a_while_read_loop() {
    let server = MockServer::start().await;
    mock_noul(&server, json!({"state": "first"})).await;
    mock_noul(&server, json!({"state": "second"})).await;
    let config = config_for(&server);
    let mut input = NamedTempFile::new().unwrap();
    input.write_all(b"first\nsecond\n").unwrap();

    let output = StdCommand::new("bash")
        .args([
            "-c",
            r#"while IFS= read -r line; do
                "$1" ask noul 'Is it urgent?' --state "$line" --config "$2" || exit 1
            done"#,
            "gutcheck-loop",
        ])
        .arg(assert_cmd::cargo::cargo_bin("gutcheck"))
        .arg(config.path())
        .env("TYPESAFE_API_KEY", "test-key")
        .stdin(File::open(input.path()).unwrap())
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"0.91\n0.91\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn usage_spec_matches_the_executable() {
    gutcheck()
        .arg("__usage_spec__")
        .env_remove("TYPESAFE_API_KEY")
        .assert()
        .success()
        .stdout(include_str!("../gutcheck.usage.kdl"));
}
