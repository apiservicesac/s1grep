mod commands;
mod model_locator;
mod settings;

use clap::{Parser, Subcommand};
use s1grep::runtime_library::RuntimeLibrary;

use commands::bench::BenchCommand;
use commands::decide::DecideCommand;
use commands::eval::EvalCommand;
use commands::profile::ProfileCommand;
use commands::rerank_eval::RerankEvalCommand;
use commands::units::UnitsCommand;

/// Development tools for s1grep: the exam, benchmarks and model checks. Not part of the release.
#[derive(Parser)]
#[command(name = "s1-lab", version)]
struct Lab {
    #[command(subcommand)]
    command: LabCommand,
}

#[derive(Subcommand)]
enum LabCommand {
    /// Run the search exam: retriever, judge and fused top-1/top-5 per language
    Eval(EvalCommand),
    /// Rerank fixed candidate lists with the judge only
    RerankEval(RerankEvalCommand),
    /// Print the functions s1grep would index, one JSON line each
    Units(UnitsCommand),
    /// Time the judge at several thread counts and sequence lengths
    Bench(BenchCommand),
    /// Run the judge on JSON states read from stdin
    Decide(DecideCommand),
    /// Time the parts of a search that do not use the models, on an existing index
    Profile(ProfileCommand),
}

fn main() -> anyhow::Result<()> {
    RuntimeLibrary::load()?;
    match Lab::parse().command {
        LabCommand::Eval(command) => command.run(),
        LabCommand::RerankEval(command) => command.run(),
        LabCommand::Units(command) => command.run(),
        LabCommand::Bench(command) => command.run(),
        LabCommand::Decide(command) => command.run(),
        LabCommand::Profile(command) => command.run(),
    }
}
