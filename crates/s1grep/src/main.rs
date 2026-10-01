use clap::{Parser, Subcommand};

use s1grep::commands::doctor::DoctorCommand;
use s1grep::commands::index::IndexCommand;
use s1grep::commands::mcp::McpCommand;
use s1grep::commands::search::SearchArgs;
use s1grep::commands::serve::ServeCommand;
use s1grep::commands::setup::SetupCommand;
use s1grep::commands::skill::SkillCommand;
use s1grep::commands::status::StatusCommand;
use s1grep::commands::stop::StopCommand;
use s1grep::runtime_library::RuntimeLibrary;

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
    /// Stop the background process that keeps the models loaded (the next search starts it again)
    Stop(StopCommand),
    #[command(hide = true)]
    Serve(ServeCommand),
    /// Run as an MCP server for coding agents (stdio)
    Mcp(McpCommand),
    /// Print or install the agent skill for Claude Code
    Skill(SkillCommand),
    /// Index a repository ahead of the first search
    Index(IndexCommand),
    #[command(hide = true)]
    Search(SearchArgs),
}

fn main() -> anyhow::Result<()> {
    RuntimeLibrary::load()?;
    let cli = Cli::parse();
    match cli.command {
        None => cli.search.run(),
        Some(Command::Setup(command)) => command.run(),
        Some(Command::Status(command)) => command.run(),
        Some(Command::Doctor(command)) => command.run(),
        Some(Command::Stop(command)) => command.run(),
        Some(Command::Serve(command)) => command.run(),
        Some(Command::Mcp(command)) => command.run(),
        Some(Command::Skill(command)) => command.run(),
        Some(Command::Index(command)) => command.run(),
        Some(Command::Search(command)) => command.run(),
    }
}
