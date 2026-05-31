use anyhow::Result;
use clap::Parser;
use clickweave_cli::args::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize a minimal tracing subscriber (stderr, WARN by default).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let exit_code = match cli.command {
        Command::Run(args) => clickweave_cli::commands::run::execute(args).await?,
        Command::RunSkill(args) => clickweave_cli::commands::run_skill::execute(args).await?,
        Command::Skills(args) => clickweave_cli::commands::skills::execute(args).await?,
        Command::Runs(args) => clickweave_cli::commands::runs::execute(args).await?,
    };

    std::process::exit(exit_code);
}
