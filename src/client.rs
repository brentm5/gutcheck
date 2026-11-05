//! The HTTP client: authentication, retries and the overall timeout.
//! See `docs/api-client.md`.

use std::collections::BTreeMap;
use std::env::{self, VarError};
use std::error::Error as _;
use std::hash::{BuildHasher, RandomState};
use std::time::{Duration, Instant, SystemTime};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::Value;

use crate::api::{
    Answer, BadRequestBody, Question, SystemOneRequest, SystemOneResponse, ValidationBody,
};
use crate::checks::{check_body_size, check_request};
use crate::config::Settings;
use crate::error::{Error, HttpDetails};

const API_KEY_VAR: &str = "TYPESAFE_API_KEY";

/// Reads the API key from `TYPESAFE_API_KEY`, the only place it can come from.
pub fn api_key_from_env() -> Result<String, Error> {
    match env::var(API_KEY_VAR) {
        Ok(key) if !key.is_empty() => Ok(key),
        Err(VarError::NotUnicode(_)) => Err(invalid_key()),
        Ok(_) | Err(VarError::NotPresent) => Err(Error::Usage(format!("{API_KEY_VAR} is not set"))),
    }
}

fn invalid_key() -> Error {
    Error::Usage(format!("{API_KEY_VAR} contains invalid characters"))
}

/// The wait between attempts. Fixed for users; only tests shorten it.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(500),
            max: Duration::from_secs(5),
        }
    }
}

impl Backoff {
    /// The wait after failed attempt number `attempt` (counting from 0).
    ///
    /// `random` is in `0.0..1.0` and takes off up to 25%. A longer `retry_after`
    /// from the server replaces the result.
    fn delay(&self, attempt: u32, retry_after: Option<Duration>, random: f64) -> Duration {
        let doubled = self
            .initial
            .saturating_mul(2u32.saturating_pow(attempt))
            .min(self.max);
        let jittered = doubled.mul_f64(1.0 - 0.25 * random);
        retry_after.map_or(jittered, |wait| wait.max(jittered))
    }
}

/// A client for the TypeSafe API.
pub struct Client {
    http: reqwest::Client,
    auth: HeaderValue,
    base_url: String,
    model: String,
    retries: u32,
    timeout: Duration,
    backoff: Backoff,
}

impl Client {
    pub fn new(settings: &Settings, api_key: &str) -> Result<Self, Error> {
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| invalid_key())?;
        auth.set_sensitive(true);

        // Redirects aren't followed: a 2xx from somewhere else (a login page, say)
        // says nothing about the key, and shouldn't receive it.
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|e| Error::Usage(format!("could not start the HTTP client: {e}")))?;

        Ok(Self {
            http,
            auth,
            base_url: settings.base_url.as_str().trim_end_matches('/').to_owned(),
            model: settings.model.clone(),
            retries: settings.retries,
            timeout: settings.timeout,
            backoff: Backoff::default(),
        })
    }

    /// Replaces the backoff timing so tests don't wait for real delays.
    #[doc(hidden)]
    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    /// Checks that the API accepts the key, using `GET /v1/models`, which needs
    /// a valid key but doesn't run the model.
    pub async fn validate_auth(&self) -> Result<(), Error> {
        let url = format!("{}/v1/models", self.base_url);
        self.send(|| self.http.get(&url)).await.map(drop)
    }

    /// Asks one question about `state` and returns its answer. Local checks run
    /// first, so a request that fails them is never sent.
    pub async fn ask_question(&self, state: Value, question: Question) -> Result<Answer, Error> {
        let id = question.kind();
        let request = SystemOneRequest {
            model: self.model.clone(),
            state,
            questions: BTreeMap::from([(id.to_owned(), question)]),
        };
        check_request(&request)?;
        let body = serde_json::to_string(&request)
            .map_err(|e| Error::Usage(format!("could not encode the request: {e}")))?;
        check_body_size(body.len())?;

        let url = format!("{}/v1/systemone", self.base_url);
        let response = self
            .send(|| {
                self.http
                    .post(&url)
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.clone())
            })
            .await
            .map_err(point_to_auth_validate)?;
        take_answer(response, id)
    }

    /// Sends a request with retries, all within the overall timeout. Dropping
    /// the future when the timeout fires cancels the request in progress.
    async fn send(&self, request: impl Fn() -> RequestBuilder) -> Result<HttpDetails, Error> {
        tokio::time::timeout(self.timeout, self.send_with_retries(request))
            .await
            .unwrap_or(Err(Error::Timeout(self.timeout)))
    }

    /// Returns the status and body of the first successful response. The body is
    /// read here so that reading it counts toward the timeout and gets retried.
    async fn send_with_retries(
        &self,
        request: impl Fn() -> RequestBuilder,
    ) -> Result<HttpDetails, Error> {
        let start = Instant::now();
        let mut attempt = 0;

        loop {
            let (error, retry_after) =
                match request().header(AUTHORIZATION, &self.auth).send().await {
                    Ok(response) if response.status().is_success() => {
                        let status = response.status().as_u16();
                        match response.text().await {
                            Ok(body) => return Ok(HttpDetails { status, body }),
                            Err(e) => (network_error(e), None),
                        }
                    }
                    Ok(response) => {
                        let retry_after = retry_after(response.headers());
                        let error = error_from_response(response).await;
                        if !matches!(error, Error::Temporary { .. }) {
                            return Err(error);
                        }
                        (error, retry_after)
                    }
                    Err(e) => (network_error(e), None),
                };

            if attempt >= self.retries {
                return Err(error);
            }
            let wait = self.backoff.delay(attempt, retry_after, random_fraction());
            if wait > self.timeout.saturating_sub(start.elapsed()) {
                return Err(error);
            }
            tokio::time::sleep(wait).await;
            attempt += 1;
        }
    }
}

async fn error_from_response(response: Response) -> Error {
    let status = response.status();
    let code = status.as_u16();
    let details = HttpDetails {
        status: code,
        body: response.text().await.unwrap_or_default(),
    };

    if is_retryable(status) {
        Error::Temporary {
            message: format!("API temporarily unavailable (HTTP {code})"),
            details: Some(details),
        }
    } else {
        Error::Rejected {
            message: rejection_message(code, &details.body),
            details,
        }
    }
}

const INVALID_KEY: &str = "invalid or missing API key";

/// The one-line message for a status that isn't retried. See the table in
/// `docs/api-client.md`.
fn rejection_message(code: u16, body: &str) -> String {
    let message = match code {
        400 => bad_request_message(body),
        401 => Some(INVALID_KEY.to_owned()),
        413 => Some("request body is over the API's 32 MiB limit".to_owned()),
        415 => Some("internal error: the API rejected the request encoding".to_owned()),
        422 => validation_message(body),
        _ => None,
    };
    message.unwrap_or_else(|| format!("unexpected API response (HTTP {code})"))
}

/// `detail` from a `400` body, which is a string or an object.
fn bad_request_message(body: &str) -> Option<String> {
    let body: BadRequestBody = serde_json::from_str(body).ok()?;
    match body.detail {
        Value::String(detail) => Some(one_line(&detail)),
        Value::Object(_) => Some(one_line(&body.detail.to_string())),
        _ => None,
    }
}

/// Each entry in a `422` body as `<loc>: <msg>`, joined so the error stays on
/// one line.
fn validation_message(body: &str) -> Option<String> {
    let body: ValidationBody = serde_json::from_str(body).ok()?;
    let entries: Vec<String> = body
        .detail
        .iter()
        .map(|entry| {
            let message = one_line(&entry.msg);
            let location = field_path(&entry.loc);
            if location.is_empty() {
                message
            } else {
                format!("{location}: {message}")
            }
        })
        .collect();
    (!entries.is_empty()).then(|| entries.join("; "))
}

/// A `loc` as a dotted path, without the leading `body` the caller never wrote.
fn field_path(loc: &[Value]) -> String {
    let path = match loc.first().and_then(Value::as_str) {
        Some("body") => &loc[1..],
        _ => loc,
    };
    let parts: Vec<String> = path
        .iter()
        .map(|part| match part {
            Value::String(name) => name.clone(),
            other => other.to_string(),
        })
        .collect();
    one_line(&parts.join("."))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Adds the pointer that `gutcheck auth validate` itself shouldn't print.
fn point_to_auth_validate(error: Error) -> Error {
    match error {
        Error::Rejected { details, .. } if details.status == 401 => Error::Rejected {
            message: format!("{INVALID_KEY}; check it with `gutcheck auth validate`"),
            details,
        },
        other => other,
    }
}

/// Pulls the answer for question `id` out of a successful response.
fn take_answer(details: HttpDetails, id: &str) -> Result<Answer, Error> {
    let answer = serde_json::from_str::<SystemOneResponse>(&details.body)
        .ok()
        .and_then(|mut response| response.answers.remove(id));
    answer.ok_or_else(|| Error::Rejected {
        message: "unexpected API response: it has no usable answer".to_owned(),
        details,
    })
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

/// Describes a failed send on one line, including the underlying cause.
/// The URL is left out because a gateway's path prefix could contain a token.
fn network_error(error: reqwest::Error) -> Error {
    let error = error.without_url();
    let mut message = format!("network error: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    Error::Temporary {
        message,
        details: None,
    }
}

/// How long the server asked us to wait.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    retry_after_at(headers, SystemTime::now())
}

/// `retry-after-ms` takes priority over `Retry-After`, which is a number of
/// seconds or an HTTP date. Unusable values and dates in the past are skipped.
fn retry_after_at(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let header = |name: &str| Some(headers.get(name)?.to_str().ok()?.trim().to_owned());
    let number = |value: &str, units_per_second: f64| {
        let number: f64 = value.parse().ok()?;
        Duration::try_from_secs_f64(number / units_per_second).ok()
    };

    header("retry-after-ms")
        .and_then(|value| number(&value, 1000.0))
        .or_else(|| {
            let value = header("retry-after")?;
            number(&value, 1.0).or_else(|| {
                let date = httpdate::parse_http_date(&value).ok()?;
                date.duration_since(now).ok()
            })
        })
}

/// A value in `0.0..1.0` for jitter. It doesn't need to be high quality, and the
/// standard library's randomly seeded hasher saves a dependency.
fn random_fraction() -> f64 {
    let random = RandomState::new().hash_one(0u8);
    (random >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_JITTER: f64 = 0.0;

    fn delay(attempt: u32, retry_after: Option<Duration>, random: f64) -> Duration {
        Backoff::default().delay(attempt, retry_after, random)
    }

    #[test]
    fn validation_entries_drop_body_and_join_on_one_line() {
        let body = r#"{"detail":[
            {"loc":["body","questions","noul","criteria"],"msg":"Field required","type":"missing"},
            {"loc":["body","items",2],"msg":"Too\nlong","type":"too_long"},
            {"loc":[],"msg":"Bad","type":"bad"}
        ]}"#;
        assert_eq!(
            rejection_message(422, body),
            "questions.noul.criteria: Field required; items.2: Too long; Bad"
        );
    }

    #[test]
    fn bad_request_reports_string_and_object_details() {
        assert_eq!(
            rejection_message(400, r#"{"detail":"max_tokens_exceeded"}"#),
            "max_tokens_exceeded"
        );
        assert_eq!(
            rejection_message(400, r#"{"detail":{"code":"nesting"}}"#),
            r#"{"code":"nesting"}"#
        );
    }

    #[test]
    fn unreadable_error_bodies_fall_back_to_the_status() {
        for code in [400, 422] {
            assert_eq!(
                rejection_message(code, "<html>"),
                format!("unexpected API response (HTTP {code})")
            );
        }
        assert_eq!(
            rejection_message(422, r#"{"detail":[]}"#),
            "unexpected API response (HTTP 422)"
        );
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let delays: Vec<u64> = (0..6)
            .map(|attempt| delay(attempt, None, NO_JITTER).as_millis() as u64)
            .collect();
        assert_eq!(delays, [500, 1000, 2000, 4000, 5000, 5000]);
    }

    #[test]
    fn backoff_survives_huge_attempt_numbers() {
        assert_eq!(delay(u32::MAX, None, NO_JITTER), Duration::from_secs(5));
    }

    #[test]
    fn jitter_takes_off_at_most_a_quarter() {
        assert_eq!(delay(1, None, 1.0), Duration::from_millis(750));
    }

    #[test]
    fn longer_retry_after_replaces_backoff() {
        let wait = Duration::from_secs(9);
        assert_eq!(delay(0, Some(wait), NO_JITTER), wait);
    }

    #[test]
    fn shorter_retry_after_keeps_backoff() {
        let wait = Duration::from_millis(1);
        assert_eq!(delay(0, Some(wait), NO_JITTER), Duration::from_millis(500));
    }

    #[test]
    fn random_fraction_stays_in_range() {
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&random_fraction()));
        }
    }

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn reads_retry_after_headers() {
        assert_eq!(
            retry_after(&headers(&[("retry-after", "2")])),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            retry_after(&headers(&[("retry-after-ms", "250")])),
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn retry_after_ms_takes_priority() {
        assert_eq!(
            retry_after(&headers(&[("retry-after", "2"), ("retry-after-ms", "250")])),
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn unusable_retry_after_ms_falls_back_to_retry_after() {
        for value in ["-1", "soon"] {
            assert_eq!(
                retry_after(&headers(&[
                    ("retry-after-ms", value),
                    ("retry-after", "30")
                ])),
                Some(Duration::from_secs(30))
            );
        }
    }

    #[test]
    fn reads_retry_after_dates() {
        let headers = headers(&[("retry-after", "Wed, 21 Oct 2026 07:28:30 GMT")]);
        let now = httpdate::parse_http_date("Wed, 21 Oct 2026 07:28:00 GMT").unwrap();
        assert_eq!(retry_after_at(&headers, now), Some(Duration::from_secs(30)));
    }

    #[test]
    fn ignores_retry_after_dates_in_the_past() {
        let headers = headers(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]);
        let now = httpdate::parse_http_date("Wed, 21 Oct 2026 07:28:30 GMT").unwrap();
        assert_eq!(retry_after_at(&headers, now), None);
    }

    #[test]
    fn ignores_unusable_retry_after() {
        for value in ["soon", "-1", "NaN", "Wed, 21 Oct 2026"] {
            assert_eq!(retry_after(&headers(&[("retry-after", value)])), None);
        }
    }
}
