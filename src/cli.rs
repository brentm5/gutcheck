//! The executable CLI definition. Usage exports the same declarations as KDL.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use gutcheck::parse_duration;
use usage_rs::{Args, Subcommands};

#[derive(Debug, usage_rs::Cli)]
#[usage(
    bin = "gutcheck",
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    license = env!("CARGO_PKG_LICENSE"),
    repository = env!("CARGO_PKG_REPOSITORY"),
    min_usage_version = "7.3.0",
    arg_required_else_help,
    unknown_flags = "error",
    args_override_self = false,
    exit_code(0, "Command succeeded, regardless of the answer"),
    exit_code(1, "Usage, configuration, API, network, timeout or output error")
)]
pub(crate) struct Cli {
    #[usage(flatten)]
    pub global: GlobalArgs,

    #[usage(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub(crate) struct GlobalArgs {
    /// Path to a TOML config file to use instead of the default
    #[usage(long, global, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Model to use (default jev-latest)
    #[usage(long, global)]
    pub model: Option<String>,

    /// Maximum time for the whole operation, including waiting for stdin and retries (e.g. 10s)
    #[usage(long, global, value_name = "DURATION")]
    pub timeout: Option<CliDuration>,

    /// On errors, print the HTTP status and raw API response body
    #[usage(long, global)]
    pub verbose: bool,
}

#[derive(Debug, Subcommands)]
pub(crate) enum Command {
    /// Ask Jev a question about some input text
    Ask(AskArgs),
    /// Manage API authentication
    Auth(AuthArgs),
}

#[derive(Debug, Args)]
#[usage(args_override_self = false)]
pub(crate) struct AskArgs {
    #[usage(subcommand)]
    pub command: AskCommand,
}

#[derive(Debug, Args)]
#[usage(args_override_self = false)]
pub(crate) struct AuthArgs {
    #[usage(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommands)]
pub(crate) enum AskCommand {
    /// Estimate the probability that a statement is true
    Noul(NoulArgs),
}

#[derive(Debug, Args)]
#[usage(
    args_override_self = false,
    output(
        "plain",
        default,
        help = "Probability as an ordinary decimal, rounded to at most three decimal places, with trailing zeros removed"
    ),
    output(
        "boolean",
        select = "--boolean",
        help = "true only when the unrounded probability is greater than the threshold; otherwise false"
    ),
    output(
        "json",
        select = "--json",
        media_type = "application/json",
        framing = "json",
        help = "The full API answer object, without display rounding"
    ),
    example(
        "gutcheck ask noul \"The message is urgent\" --state \"Production is down\"",
        header = "Read the probability"
    ),
    example(
        "gutcheck ask noul \"The message is urgent\" --boolean --threshold 0.9 --state \"Production is down\"",
        header = "Make a boolean decision"
    )
)]
pub(crate) struct NoulArgs {
    /// The yes/no question or statement to evaluate
    pub question: String,

    /// Input text; takes precedence over stdin and leaves it unread
    ///
    /// Without --state, read nonterminal stdin as UTF-8 text until EOF, preserving
    /// trailing newlines. Waiting for stdin counts toward --timeout. An explicit
    /// empty string supplies a state; empty stdin does not.
    #[usage(long, value_name = "TEXT")]
    pub state: Option<String>,

    /// What counts as yes
    #[usage(long = "true", value_name = "TEXT")]
    pub if_true: Option<String>,

    /// What counts as no
    #[usage(long = "false", value_name = "TEXT")]
    pub if_false: Option<String>,

    /// Print true when probability > threshold, otherwise false (default cutoff 0.5)
    ///
    /// Equality prints false. Compare the unrounded API value, independently of
    /// plain-output rounding. Both true and false are successful answers.
    #[usage(long, conflicts("--json"))]
    pub boolean: bool,

    /// Boolean cutoff from 0 to 1; requires --boolean and defaults to 0.5 in that mode
    #[usage(
        long,
        value_name = "0-1",
        requires("--boolean"),
        default_if("--boolean", "0.5"),
        validate = r#"value matches "^[+-]?([0-9]+([.][0-9]*)?|[.][0-9]+)([eE][+-]?[0-9]+)?$" && float(value) >= 0 && float(value) <= 1"#,
        validate_error = "must be a number from 0 to 1"
    )]
    pub threshold: Option<Threshold>,

    /// Print the full answer object as one line of JSON, without display rounding
    #[usage(long)]
    pub json: bool,
}

#[derive(Debug, Subcommands)]
pub(crate) enum AuthCommand {
    /// Check that TYPESAFE_API_KEY is set and accepted by the API
    Validate(ValidateArgs),
}

#[derive(Debug, Args)]
#[usage(
    args_override_self = false,
    output(
        "silent",
        default,
        help = "No stdout; the exit code reports whether validation succeeded"
    )
)]
pub(crate) struct ValidateArgs {}

/// A CLI duration uses the same parser as configuration, without parser callbacks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CliDuration(pub Duration);

impl FromStr for CliDuration {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_duration(value).map(Self)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Threshold(pub f64);

impl FromStr for Threshold {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.parse() {
            Ok(threshold) if (0.0..=1.0).contains(&threshold) => Ok(Self(threshold)),
            _ => Err("must be a number from 0 to 1"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::process::{Command, Stdio};

    use super::Cli;

    #[test]
    fn portable_spec_and_native_parser_enforce_the_same_contract() {
        let spec = Cli::to_kdl();
        let cases: &[(&[&str], bool)] = &[
            (&["ask", "noul", "Urgent?"], true),
            (&["ask", "noul", "Urgent?", "--boolean"], true),
            (&["ask", "noul", "Urgent?", "--json"], true),
            (&["ask", "noul", "Urgent?", "--state", ""], true),
            (&["ask", "noul", "Urgent?", "--threshold", "0.7"], false),
            (
                &["ask", "noul", "Urgent?", "--boolean", "--threshold", "0.7"],
                true,
            ),
            (
                &["ask", "noul", "Urgent?", "--boolean", "--threshold", "abc"],
                false,
            ),
            (
                &["ask", "noul", "Urgent?", "--boolean", "--threshold", "NaN"],
                false,
            ),
            (
                &["ask", "noul", "Urgent?", "--boolean", "--threshold", "1.1"],
                false,
            ),
            (&["ask", "noul", "Urgent?", "--boolean", "--json"], false),
            (
                &["ask", "noul", "Urgent?", "--state", "a", "--state", "b"],
                false,
            ),
            (&["ask", "noul", "Urgent?", "--boolean", "--boolean"], false),
            (&["ask", "noul", "Urgent?", "--verbose", "--verbose"], false),
            (&["ask", "noul", "Urgent?", "--exit-code"], false),
            (&["ask", "choice", "Team?"], false),
            (&["auth", "validate"], true),
        ];

        // Start every portable check first so they run side by side.
        let children: Vec<_> = cases
            .iter()
            .map(|(args, _)| {
                Command::new("usage")
                    .args([
                        "explain",
                        "--format",
                        "json",
                        "--env",
                        "SPEC_TEST=1",
                        "--spec",
                        &spec,
                        "--",
                        "gutcheck",
                    ])
                    .args(*args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("Usage CLI must be on PATH: run mise install, then mise run test")
            })
            .collect();

        for ((args, expected_valid), child) in cases.iter().zip(children) {
            let argv: Vec<_> = args.iter().map(OsStr::new).collect();
            let native = Cli::parse_from(&argv);
            assert_eq!(
                native.is_ok(),
                *expected_valid,
                "native: {args:?}: {native:?}"
            );

            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let errors = report["errors"].as_array().unwrap();
            let valid = errors.is_empty() && report["refused"].is_null();
            assert_eq!(valid, *expected_valid, "portable: {args:?}: {report}");
            if args.contains(&"abc") {
                let message = errors.iter().map(ToString::to_string).collect::<String>();
                assert!(
                    message.contains("must be a number from 0 to 1"),
                    "{message}"
                );
                assert!(!message.contains("expression failed"), "{message}");
            }
        }
    }
}
