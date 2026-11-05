use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use gutcheck::{
    Answer, Client, Error, MAX_BODY_BYTES, Overrides, Question, Settings, api_key_from_env,
    check_body_size, parse_duration,
};
use tokio::io::AsyncReadExt;

/// How long `--state` waits to see whether stdin also has data.
const STDIN_CHECK: Duration = Duration::from_millis(100);

/// How often stdin is looked at during that wait.
const STDIN_CHECK_INTERVAL: Duration = Duration::from_millis(5);

/// Ask TypeSafe's Jev model quick, structured questions.
#[derive(Debug, Parser)]
#[command(name = "gutcheck", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Args)]
struct GlobalArgs {
    /// Path to a config file to use instead of the default
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Model to use (default jev-latest)
    #[arg(long, global = true)]
    model: Option<String>,

    /// Maximum time for the whole command, including retries (e.g. 10s)
    #[arg(long, global = true, value_parser = parse_duration, value_name = "DURATION")]
    timeout: Option<Duration>,

    /// On errors, print the HTTP status and raw API response body
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Ask Jev a question about some input text
    Ask {
        #[command(subcommand)]
        command: AskCommand,
    },
    /// Manage API authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AskCommand {
    /// Whether a statement is true
    Noul(NoulArgs),
}

#[derive(Debug, Args)]
struct NoulArgs {
    /// The question or statement to evaluate
    question: String,

    /// The input text to ask about (reads stdin if not given)
    #[arg(long, value_name = "TEXT")]
    state: Option<String>,

    /// What counts as yes
    #[arg(long = "true", value_name = "TEXT")]
    if_true: Option<String>,

    /// What counts as no
    #[arg(long = "false", value_name = "TEXT")]
    if_false: Option<String>,

    /// The cutoff from 0 to 1: true if the probability is at or above it, otherwise false
    #[arg(long, value_parser = parse_threshold, default_value_t = 0.5, value_name = "0-1")]
    threshold: f64,

    /// Print the full answer as JSON instead of true or false
    #[arg(long, conflicts_with = "threshold")]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Check that the API key works
    Validate,
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    run_without_waiting_for_blocking_work(runtime, cli_main())
}

/// Runs `future`, then shuts down without waiting for blocking tasks. A DNS lookup
/// runs on one and can't be cancelled, and waiting for it at exit would let the
/// process outlive `--timeout`.
fn run_without_waiting_for_blocking_work<T>(
    runtime: tokio::runtime::Runtime,
    future: impl Future<Output = T>,
) -> T {
    let output = runtime.block_on(future);
    runtime.shutdown_background();
    output
}

async fn cli_main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            // Clap exits 2 on usage errors, but every error here exits 1.
            return if error.use_stderr() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };

    let verbose = cli.global.verbose;
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            if verbose && let Some(details) = error.http_details() {
                eprintln!("HTTP status: {}", details.status);
                eprintln!("response body: {}", details.body);
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Error> {
    let settings = Settings::load(
        cli.global.config.as_deref(),
        Overrides {
            model: cli.global.model,
            timeout: cli.global.timeout,
        },
    )?;

    match cli.command {
        Command::Ask {
            command: AskCommand::Noul(args),
        } => {
            let client = Client::new(&settings, &api_key_from_env()?)?;
            // The timeout covers the whole command, so it includes waiting on stdin.
            let deadline = client.deadline();
            let state = tokio::time::timeout_at(deadline, read_state(args.state))
                .await
                .unwrap_or(Err(Error::Timeout(settings.timeout)))?;
            let question = Question::noul(args.question, args.if_true, args.if_false);
            let answer = client
                .ask_question(state.into(), question, deadline)
                .await?;
            let output = if args.json {
                answer_json(&answer)?
            } else {
                noul_text(&answer, args.threshold)
            };
            print_line(&output)
        }
        Command::Auth {
            command: AuthCommand::Validate,
        } => {
            let client = Client::new(&settings, &api_key_from_env()?)?;
            client.validate_auth().await
        }
    }
}

/// Picks the state from `--state` or stdin. See `docs/cli-interface.md`.
async fn read_state(flag: Option<String>) -> Result<String, Error> {
    let stdin_is_terminal = io::stdin().is_terminal();
    match flag {
        Some(_) if !stdin_is_terminal && stdin_has_data().await => Err(Error::Usage(
            "state given with both --state and stdin; use one".to_owned(),
        )),
        Some(state) => Ok(state),
        None if !stdin_is_terminal => {
            // One byte past the limit is enough to know it's too large.
            let limit = MAX_BODY_BYTES as u64 + 1;
            let mut bytes = Vec::new();
            tokio::io::stdin()
                .take(limit)
                .read_to_end(&mut bytes)
                .await
                .map_err(|e| Error::Usage(format!("could not read stdin: {e}")))?;
            if bytes.is_empty() {
                return Err(no_state());
            }
            check_body_size(bytes.len())?;
            String::from_utf8(bytes)
                .map_err(|_| Error::Usage("stdin is not valid UTF-8".to_owned()))
        }
        None => Err(no_state()),
    }
}

/// Whether stdin delivers data soon. Scripts, cron jobs and agents often run with
/// a stdin that isn't a terminal and never sends anything, so that alone doesn't
/// count as giving state, and waiting for it to close could hang forever.
///
/// This asks how many bytes are waiting instead of reading, because stdin may
/// belong to the script that started this command. Polling for readiness wouldn't
/// do: `/dev/null`, a closed pipe and a file at its end are all "ready" with no data.
async fn stdin_has_data() -> bool {
    let start = tokio::time::Instant::now();
    loop {
        match rustix::io::ioctl_fionread(io::stdin()) {
            Ok(0) if start.elapsed() < STDIN_CHECK => {
                tokio::time::sleep(STDIN_CHECK_INTERVAL).await;
            }
            Ok(0) | Err(_) => return false,
            Ok(_) => return true,
        }
    }
}

fn no_state() -> Error {
    Error::Usage("no state given: pass --state or pipe it to stdin".to_owned())
}

/// Parses `--threshold`. `NaN` fails the range check too.
fn parse_threshold(value: &str) -> Result<f64, String> {
    match value.parse() {
        Ok(threshold) if (0.0..=1.0).contains(&threshold) => Ok(threshold),
        _ => Err("must be a number from 0 to 1".to_owned()),
    }
}

/// `true` if the probability reaches `threshold`, otherwise `false`.
fn noul_text(answer: &Answer, threshold: f64) -> String {
    (answer.probability() >= threshold).to_string()
}

fn answer_json(answer: &Answer) -> Result<String, Error> {
    serde_json::to_string(answer)
        .map_err(|e| Error::Output(format!("could not encode the output: {e}")))
}

fn print_line(line: &str) -> Result<(), Error> {
    writeln!(io::stdout(), "{line}")
        .map_err(|e| Error::Output(format!("could not write the output: {e}")))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::leans_true(0.6, 0.5, "true")]
    #[case::leans_false(0.4, 0.5, "false")]
    #[case::exact_middle_at_the_default(0.5, 0.5, "true")]
    #[case::on_the_cutoff(0.9, 0.9, "true")]
    #[case::below_a_high_cutoff(0.89, 0.9, "false")]
    #[case::above_a_low_cutoff(0.02, 0.01, "true")]
    #[case::below_a_low_cutoff(0.005, 0.01, "false")]
    #[case::zero_cutoff_is_always_true(0.0, 0.0, "true")]
    #[case::top_cutoff_needs_certainty(0.999, 1.0, "false")]
    fn noul_text_compares_with_the_cutoff(
        #[case] probability: f64,
        #[case] threshold: f64,
        #[case] expected: &str,
    ) {
        let answer = Answer::Noul {
            noul: serde_json::Number::from_f64(probability).unwrap(),
            extra: serde_json::Map::new(),
        };
        assert_eq!(noul_text(&answer, threshold), expected);
    }

    #[test]
    fn exit_does_not_wait_for_blocking_work() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let start = Instant::now();

        run_without_waiting_for_blocking_work(runtime, async {
            tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(30)));
            tokio::task::yield_now().await;
        });

        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
