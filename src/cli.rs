use clap::{Parser, Subcommand};

use crate::{Result, app_server, repl};

#[derive(Debug, Parser)]
#[command(name = "cawir", about = "Coding Agent Written in Rust")]
struct Cli {
    #[arg(long, value_name = "ID", conflicts_with = "continue_session")]
    resume: Option<String>,

    #[arg(long = "continue", conflicts_with = "resume")]
    continue_session: bool,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(name = "app-server")]
    AppServer,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(CliCommand::AppServer) => app_server::run_stdio().await,
        None => {
            repl::run(repl::ReplOptions {
                resume: cli.resume,
                continue_session: cli.continue_session,
            })
            .await
        }
    }
}
