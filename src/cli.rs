use clap::{Args, Parser, Subcommand};

use crate::{Result, app_server, exec, repl, tui};

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
    AppServer(AppServerArgs),

    #[command(name = "exec")]
    Exec(ExecArgs),

    #[command(name = "tui")]
    Tui(TuiArgs),
}

#[derive(Debug, Args)]
struct AppServerArgs {
    #[arg(
        long,
        value_name = "ADDR",
        help = "Accept one App Server WebSocket client at this address instead of stdio"
    )]
    websocket: Option<String>,
}

#[derive(Debug, Args)]
struct ExecArgs {
    #[arg(
        long,
        value_name = "PROVIDER",
        conflicts_with = "resume",
        help = "Create a new exec session with this provider"
    )]
    provider: Option<String>,

    #[arg(
        long,
        value_name = "MODEL",
        conflicts_with = "resume",
        help = "Create a new exec session with this model"
    )]
    model: Option<String>,

    #[arg(long, value_name = "ID", help = "Resume an existing saved session")]
    resume: Option<String>,

    #[arg(long, help = "Emit JSONL events and turn result")]
    json: bool,

    #[arg(
        long,
        help = "Approve all App Server approval requests; default is to deny"
    )]
    approve: bool,

    #[arg(
        value_name = "PROMPT",
        required = true,
        num_args = 1..,
        trailing_var_arg = true,
        help = "Prompt to submit as one App Server turn"
    )]
    prompt: Vec<String>,
}

#[derive(Debug, Args)]
struct TuiArgs {
    #[arg(
        long,
        value_name = "PROVIDER",
        conflicts_with = "resume",
        help = "Create a new TUI session with this provider"
    )]
    provider: Option<String>,

    #[arg(
        long,
        value_name = "MODEL",
        conflicts_with = "resume",
        help = "Create a new TUI session with this model"
    )]
    model: Option<String>,

    #[arg(long, value_name = "ID", help = "Resume an existing saved session")]
    resume: Option<String>,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(CliCommand::AppServer(args)) => match args.websocket {
            Some(address) => app_server::run_websocket(&address).await,
            None => app_server::run_stdio().await,
        },
        Some(CliCommand::Exec(args)) => exec::run(exec::ExecOptions {
            prompt: args.prompt.join(" "),
            provider: args.provider,
            model: args.model,
            resume: args.resume,
            json_output: args.json,
            approve: args.approve,
        }),
        Some(CliCommand::Tui(args)) => tui::run(tui::TuiOptions {
            provider: args.provider,
            model: args.model,
            resume: args.resume,
        }),
        None => {
            repl::run(repl::ReplOptions {
                resume: cli.resume,
                continue_session: cli.continue_session,
            })
            .await
        }
    }
}
