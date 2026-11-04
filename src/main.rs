use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use gutcheck::{Client, Error, Overrides, Settings, api_key_from_env, parse_duration};

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
    /// Manage API authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
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
        Command::Auth {
            command: AuthCommand::Validate,
        } => {
            let client = Client::new(&settings, &api_key_from_env()?)?;
            client.validate_auth().await?;
            // The key is valid whether or not anything is still reading stdout.
            let _ = writeln!(io::stdout(), r#"{{"valid": true}}"#);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

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
