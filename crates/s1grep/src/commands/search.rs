use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Instant;

use clap::Args;

use crate::backend::{Answered, SearchBackend};
use crate::models::ModelDirectory;
use crate::report::TextReport;
use crate::service::SearchRequest;

#[derive(Args)]
pub struct SearchArgs {
    /// What the code does, in English or Spanish, e.g. "where do we retry a failed payment"
    pub query: Option<String>,
    /// Repository or folder to search
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Results to show
    #[arg(long, short = 'n', default_value_t = SearchRequest::DEFAULT_TOP)]
    pub top: usize,
    /// Candidates the judge reads; more is slower and slightly more accurate
    #[arg(long, default_value_t = 5)]
    pub judge_top: usize,
    /// Rank by embeddings only, without the judge
    #[arg(long)]
    pub no_judge: bool,
    /// Also search tests/ and migrations/
    #[arg(long)]
    pub include_tests: bool,
    /// Machine-readable output, for scripts and agents
    #[arg(long)]
    pub json: bool,
    /// Lines of code shown under each result
    #[arg(long, default_value_t = 6)]
    pub lines: usize,
    /// Run in this process even when `s1grep serve` is running
    #[arg(long)]
    pub no_server: bool,
    #[arg(long, hide = true)]
    pub threads: Option<usize>,
    #[command(flatten)]
    pub models: ModelDirectory,
}

impl SearchArgs {
    pub fn run(self) -> anyhow::Result<()> {
        let Some(query) = self.query.as_deref().filter(|query| !query.trim().is_empty()) else {
            anyhow::bail!("say what the code does, e.g. s1grep \"where do we retry a failed payment\" .");
        };
        let started = Instant::now();
        let judge_top = if self.no_judge { 0 } else { self.judge_top };
        let request = SearchRequest::new(query, &self.path, self.top, judge_top, self.include_tests)?;
        let mut backend = SearchBackend::new(self.models.clone(), self.threads, !self.no_server);
        let (response, answered) = backend.search(&request)?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&response)?);
            return Ok(());
        }
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        print!(
            "{}",
            TextReport {
                response: &response,
                preview_lines: self.lines,
                color
            }
            .render()
        );
        let source = match answered {
            Answered::Server => "s1grep serve",
            Answered::Local => "this process (run `s1grep serve` to keep the models loaded)",
        };
        eprintln!(
            "{} functions · {} files re-indexed · search {:.2} s · total {:.2} s · {source}",
            response.functions,
            response.reindexed_files,
            response.search_seconds,
            started.elapsed().as_secs_f64()
        );
        Ok(())
    }
}
