mod backend;
mod commands;
mod hub;
mod indexer;
mod model_locator;
mod models;
mod progress;
mod project;
mod report;
mod runtime_library;
mod searcher;
mod server;
mod service;
mod settings;

use clap::{Parser, Subcommand};

use commands::bench::BenchCommand;
use commands::decide::DecideCommand;
use commands::doctor::DoctorCommand;
use commands::eval::EvalCommand;
use commands::index::IndexCommand;
use commands::mcp::McpCommand;
use commands::rerank_eval::RerankEvalCommand;
use commands::search::SearchArgs;
use commands::serve::ServeCommand;
use commands::setup::SetupCommand;
use commands::skill::SkillCommand;
use commands::status::StatusCommand;
use commands::units::UnitsCommand;
use runtime_library::RuntimeLibrary;

#[derive(Parser)]
#[command(
    name = "s1grep",
    version,
    about = "Find code by asking what it does, in English or Spanish. Everything runs on your machine.",
    override_usage = "s1grep \"<what the code does>\" [PATH] [OPTIONS]\n       s1grep <COMMAND>",
    after_help = "Examples:\n  s1grep setup\n  s1grep \"where do we retry a failed payment\" ~/work/shop\n  s1grep \"dónde se valida el token\" . -n 10 --json",
    args_conflicts_with_subcommands = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    search: SearchArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Download the models (once, about 2.4 GB)
    Setup(SetupCommand),
    /// Show the server, the models and how far each project is indexed
    Status(StatusCommand),
    /// Check that the models, the server and the index are in place
    Doctor(DoctorCommand),
    /// Keep the models loaded so that searches answer in about a second
    Serve(ServeCommand),
    /// Run as an MCP server for coding agents (stdio)
    Mcp(McpCommand),
    /// Print or install the agent skill for Claude Code
    Skill(SkillCommand),
    /// Index a repository ahead of the first search
    Index(IndexCommand),
    #[command(hide = true)]
    Search(SearchArgs),
    #[command(hide = true)]
    Eval(EvalCommand),
    #[command(hide = true)]
    RerankEval(RerankEvalCommand),
    #[command(hide = true)]
    Units(UnitsCommand),
    #[command(hide = true)]
    Bench(BenchCommand),
    #[command(hide = true)]
    Decide(DecideCommand),
}

fn main() -> anyhow::Result<()> {
    RuntimeLibrary::load()?;
    let cli = Cli::parse();
    match cli.command {
        None => cli.search.run(),
        Some(Command::Setup(command)) => command.run(),
        Some(Command::Status(command)) => command.run(),
        Some(Command::Doctor(command)) => command.run(),
        Some(Command::Serve(command)) => command.run(),
        Some(Command::Mcp(command)) => command.run(),
        Some(Command::Skill(command)) => command.run(),
        Some(Command::Index(command)) => command.run(),
        Some(Command::Search(command)) => command.run(),
        Some(Command::Eval(command)) => command.run(),
        Some(Command::RerankEval(command)) => command.run(),
        Some(Command::Units(command)) => command.run(),
        Some(Command::Bench(command)) => command.run(),
        Some(Command::Decide(command)) => command.run(),
    }
}
