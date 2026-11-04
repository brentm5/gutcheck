//! The error type shared by the CLI and MCP front ends.

use std::time::Duration;

/// The HTTP status and raw body of a failed API response, shown by `--verbose`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpDetails {
    pub status: u16,
    pub body: String,
}

/// Every failure, grouped by the categories in `docs/api-client.md`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Bad input or configuration, caught before any request is sent.
    #[error("{0}")]
    Usage(String),

    /// The API rejected the request. Retrying wouldn't help.
    #[error("{message}")]
    Rejected {
        message: String,
        details: HttpDetails,
    },

    /// A server or network problem that was still failing when retries ran out.
    #[error("{message}")]
    Temporary {
        message: String,
        details: Option<HttpDetails>,
    },

    /// The whole operation went past the configured `timeout`.
    #[error("timed out after {0:?}")]
    Timeout(Duration),
}

impl Error {
    /// The HTTP response behind this error, if there was one.
    pub fn http_details(&self) -> Option<&HttpDetails> {
        match self {
            Error::Rejected { details, .. } => Some(details),
            Error::Temporary { details, .. } => details.as_ref(),
            Error::Usage(_) | Error::Timeout(_) => None,
        }
    }
}
