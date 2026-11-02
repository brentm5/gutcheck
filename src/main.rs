use clap::Parser;

/// Ask TypeSafe's Jev model quick, structured questions.
#[derive(Debug, Parser)]
#[command(name = "gutcheck", version, about, arg_required_else_help = true)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
