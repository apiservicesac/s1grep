mod commands;
mod model_locator;
mod models;
mod runtime_library;
mod searcher;

use clap::{Parser, Subcommand};

use commands::bench::BenchCommand;
use commands::decide::DecideCommand;
use commands::eval::EvalCommand;
use commands::index::IndexCommand;
use commands::models::ModelsCommand;
use commands::rerank_eval::RerankEvalCommand;
use commands::search::SearchCommand;
use commands::units::UnitsCommand;
use runtime_library::RuntimeLibrary;

#[derive(Parser)]
#[command(name = "s1grep", version, about = "Find code by asking what it does")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search a repository by what the code does (indexes it on the fly)
    Search(SearchCommand),
    /// Build or refresh the index of a repository
    Index(IndexCommand),
    /// Download the models or check that they are in place
    Models(ModelsCommand),
    /// Run an exam through the full pipeline and report accuracy and latency
    Eval(EvalCommand),
    /// Judge precomputed candidates (JSON lines) and report accuracy and latency per setting
    RerankEval(RerankEvalCommand),
    /// List the functions the index would store (JSON lines)
    Units(UnitsCommand),
    /// Measure decision latency on this machine
    Bench(BenchCommand),
    /// Answer typed questions (Jev format) about a state read from a file or stdin
    Decide(DecideCommand),
}

fn main() -> anyhow::Result<()> {
    RuntimeLibrary::load()?;
    match Cli::parse().command {
        Command::Search(command) => command.run(),
        Command::Index(command) => command.run(),
        Command::Models(command) => command.run(),
        Command::Eval(command) => command.run(),
        Command::RerankEval(command) => command.run(),
        Command::Units(command) => command.run(),
        Command::Bench(command) => command.run(),
        Command::Decide(command) => command.run(),
    }
}
