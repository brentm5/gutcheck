//! The HTTP client: authentication, retries and the overall timeout.
//! See `docs/api-client.md`.

use std::env::{self, VarError};
use std::error::Error as _;
use std::hash::{BuildHasher, RandomState};
use std::time::{Duration, Instant, SystemTime};

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{RequestBuilder, Response, StatusCode};

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

    /// Sends a request with retries, all within the overall timeout. Dropping
    /// the future when the timeout fires cancels the request in progress.
    async fn send(&self, request: impl Fn() -> RequestBuilder) -> Result<Response, Error> {
        tokio::time::timeout(self.timeout, self.send_with_retries(request))
            .await
            .unwrap_or(Err(Error::Timeout(self.timeout)))
    }

    async fn send_with_retries(
        &self,
        request: impl Fn() -> RequestBuilder,
    ) -> Result<Response, Error> {
        let start = Instant::now();
        let mut attempt = 0;

        loop {
            let (error, retry_after) =
                match request().header(AUTHORIZATION, &self.auth).send().await {
                    Ok(response) if response.status().is_success() => return Ok(response),
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
    } else if status == StatusCode::UNAUTHORIZED {
        Error::Rejected {
            message: "invalid or missing API key".to_owned(),
            details,
        }
    } else {
        Error::Rejected {
            message: format!("unexpected API response (HTTP {code})"),
            details,
        }
    }
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
