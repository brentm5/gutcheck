use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

mod cli;
mod output;

use cli::{AskCommand, AuthCommand, Cli, Command};
use gutcheck::api::{Answer, ChoiceCriteria, NoulCriteria, Question};
use gutcheck::{
    Client, Error, MAX_BODY_BYTES, Overrides, Settings, api_key_from_env, check_body_size,
};
use tokio::io::AsyncReadExt;

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
    let argv: Vec<_> = std::env::args_os().skip(1).collect();
    let cli = match Cli::embedded_outcome(&argv) {
        usage_rs::embedded::Outcome::Parsed(cli) => cli,
        usage_rs::embedded::Outcome::Exit(exit) => {
            let result = if exit.stderr {
                let text = if exit.code == 0 {
                    None
                } else {
                    usage_error_line(&argv)
                };
                write_output(io::stderr(), &text.unwrap_or(exit.text))
            } else {
                write_output(io::stdout(), &exit.text)
            };
            if let Err(error) = result {
                eprintln!("error: {error}");
                return ExitCode::FAILURE;
            }
            // The parser's default usage-error status is 2; gutcheck uses 1 for every error.
            return if exit.code == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
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
            timeout: cli.global.timeout.map(|timeout| timeout.0),
        },
    )?;

    match cli.command {
        Command::Ask(cli::AskArgs {
            command: AskCommand::Choice(args),
        }) => {
            let criteria = choice_criteria(args.options)?;
            let question = Question::choice(args.question, criteria);
            let answer = ask(&settings, args.state, question).await?;
            let output = if args.json {
                answer_json(&answer)?
            } else {
                answer.choice().ok_or_else(wrong_answer_type)?.to_owned()
            };
            print_line(&output)
        }
        Command::Ask(cli::AskArgs {
            command: AskCommand::Noul(args),
        }) => {
            let criteria = NoulCriteria {
                if_true: args.if_true,
                if_false: args.if_false,
            };
            let question = Question::noul(args.question, criteria);
            let answer = ask(&settings, args.state, question).await?;
            let output = if args.json {
                answer_json(&answer)?
            } else {
                let probability = answer.probability().ok_or_else(wrong_answer_type)?;
                if args.boolean {
                    let threshold = args
                        .threshold
                        .expect("--boolean supplies a threshold through default_if");
                    boolean_text(probability, threshold.0)
                } else {
                    output::number_text(probability)
                }
            };
            print_line(&output)
        }
        Command::Auth(cli::AuthArgs {
            command: AuthCommand::Validate(_),
        }) => {
            let client = Client::new(&settings, &api_key_from_env()?)?;
            client.validate_auth().await
        }
    }
}

/// The shared steps of every `ask` subcommand: read the state, then send the
/// one question. See `docs/cli-interface.md`.
async fn ask(
    settings: &Settings,
    state: Option<String>,
    question: Question,
) -> Result<Answer, Error> {
    let client = Client::new(settings, &api_key_from_env()?)?;
    // The timeout covers the whole command, so it includes waiting on stdin.
    let deadline = client.deadline();
    let state = client.within(deadline, read_state(state)).await?;
    client.ask_question(state.into(), question, deadline).await
}

/// Builds `choice` criteria in flag order. A key may appear only once, whether
/// bare or with a description. See `docs/cli-interface.md`.
fn choice_criteria(options: Vec<cli::ChoiceOption>) -> Result<ChoiceCriteria, Error> {
    let mut seen = HashSet::new();
    let mut criteria = Vec::with_capacity(options.len());
    for option in options {
        if !seen.insert(option.key.clone()) {
            return Err(Error::Usage(format!(
                "duplicate option key {:?}: each -o key may be given only once",
                option.key
            )));
        }
        criteria.push((option.key, option.description));
    }
    Ok(ChoiceCriteria(criteria))
}

/// The client only returns an answer of the question's type, so this is a bug.
fn wrong_answer_type() -> Error {
    Error::Internal("the answer does not match the question type".to_owned())
}

/// The one-line `error: ...` message for a command line the parser rejected, or
/// `None` when the parser's response isn't an error message, like the help shown
/// when a command or subcommand is missing. See `docs/output-and-exit-codes.md`.
///
/// The parser's own rendering adds a usage line and hints after a blank line,
/// and colours the text on a terminal. So this parses again and renders the error
/// without colour, from words whose line breaks are spaces. Then a blank line can
/// only end the message, never come from an argument, and the paragraph before it
/// is the whole message. A message that lists items on their own lines, such as
/// missing flags, is joined with spaces.
fn usage_error_line(argv: &[OsString]) -> Option<String> {
    let words: Vec<OsString> = argv.iter().map(|word| without_line_breaks(word)).collect();
    let words: Vec<&OsStr> = words.iter().map(OsString::as_os_str).collect();
    let error = Cli::parse_from(&words).err()?;
    if matches!(
        error,
        usage_rs::Error::Help { .. }
            | usage_rs::Error::HelpAll { .. }
            | usage_rs::Error::MissingArgsHelp { .. }
            | usage_rs::Error::MissingSubcommand
            | usage_rs::Error::Version { .. }
    ) {
        return None;
    }
    let rendered = usage_rs::render_failure_plain(Cli::spec(), &words, &error);
    let message: Vec<&str> = rendered
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .map(str::trim)
        .collect();
    Some(format!("{}\n", message.join(" ")))
}

/// `word` with each run of line breaks replaced by one space. A word without
/// any is returned as is, so non-UTF-8 words parse exactly as before.
fn without_line_breaks(word: &OsStr) -> OsString {
    let is_break = |c: char| c == '\n' || c == '\r';
    let text = word.to_string_lossy();
    if !text.contains(is_break) {
        return word.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut in_break = false;
    for c in text.chars() {
        if is_break(c) {
            if !in_break {
                out.push(' ');
            }
            in_break = true;
        } else {
            out.push(c);
            in_break = false;
        }
    }
    out.into()
}

/// Picks the state from `--state` or stdin. See `docs/cli-interface.md`.
async fn read_state(flag: Option<String>) -> Result<String, Error> {
    match flag {
        Some(state) => Ok(state),
        None if !io::stdin().is_terminal() => {
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

fn no_state() -> Error {
    Error::Usage("no state given: pass --state or pipe it to stdin".to_owned())
}

/// `true` if the unrounded probability exceeds `threshold`, otherwise `false`.
fn boolean_text(probability: f64, threshold: f64) -> String {
    (probability > threshold).to_string()
}

fn answer_json(answer: &Answer) -> Result<String, Error> {
    serde_json::to_string(answer)
        .map_err(|e| Error::Output(format!("could not encode the output: {e}")))
}

fn print_line(line: &str) -> Result<(), Error> {
    write_output(io::stdout(), &format!("{line}\n"))
}

fn write_output(mut stream: impl Write, text: &str) -> Result<(), Error> {
    stream
        .write_all(text.as_bytes())
        .map_err(|e| Error::Output(format!("could not write the output: {e}")))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::leans_true(0.6, 0.5, "true")]
    #[case::leans_false(0.4, 0.5, "false")]
    #[case::exact_middle_at_the_default(0.5, 0.5, "false")]
    #[case::on_the_cutoff(0.9, 0.9, "false")]
    #[case::below_a_high_cutoff(0.89, 0.9, "false")]
    #[case::above_a_low_cutoff(0.02, 0.01, "true")]
    #[case::below_a_low_cutoff(0.005, 0.01, "false")]
    #[case::zero_at_zero_cutoff(0.0, 0.0, "false")]
    #[case::one_at_one_cutoff(1.0, 1.0, "false")]
    #[case::top_cutoff_needs_certainty(0.999, 1.0, "false")]
    fn boolean_text_compares_with_the_cutoff(
        #[case] probability: f64,
        #[case] threshold: f64,
        #[case] expected: &str,
    ) {
        assert_eq!(boolean_text(probability, threshold), expected);
    }

    #[rstest]
    #[case::hint_dropped(
        &["ask", "choice", "Q", "-o", "=x"],
        Some("error: invalid value '=x' for '--option': option key must not be empty\n")
    )]
    #[case::list_joined(
        &["ask", "choice", "Q"],
        Some("error: the following required arguments were not provided: --option\n")
    )]
    #[case::blank_line_in_a_value(
        &["ask", "choice", "Q", "-o", "=a\n\nb"],
        Some("error: invalid value '=a b' for '--option': option key must not be empty\n")
    )]
    #[case::missing_command_help(&["ask"], None)]
    #[case::requested_help(&["ask", "choice", "--help"], None)]
    #[case::valid(&["ask", "choice", "Q", "-o", "a"], None)]
    fn usage_errors_become_one_line(#[case] args: &[&str], #[case] expected: Option<&str>) {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        assert_eq!(usage_error_line(&argv).as_deref(), expected);
    }

    #[rstest]
    #[case::unchanged("plain text", "plain text")]
    #[case::newline("a\nb", "a b")]
    #[case::crlf_run("a\r\n\r\nb", "a b")]
    fn line_breaks_become_one_space(#[case] word: &str, #[case] expected: &str) {
        assert_eq!(without_line_breaks(OsStr::new(word)), OsStr::new(expected));
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
