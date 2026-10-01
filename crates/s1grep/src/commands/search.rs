use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Instant;

use clap::Args;

use crate::backend::{Answered, SearchBackend};
use crate::models::ModelDirectory;
use crate::progress::{ProgressDisplay, Units};
use crate::report::TextReport;
use crate::service::{FileFilters, SearchRequest, SearchResponse};
use crate::settings::SearchSettings;

/// One-off exclusions on top of the ignore files: shared by `search` and `index`.
#[derive(Args, Clone, Default)]
pub struct FilterArgs {
    /// Leave out files or folders matching a gitignore-style pattern, e.g. --exclude 'legacy/' (repeatable).
    /// Lasting rules belong in ~/.config/s1grep/ignore or in a .s1grepignore file
    #[arg(long = "exclude", value_name = "PATTERN")]
    pub excludes: Vec<String>,
}

impl FilterArgs {
    pub fn filters(&self) -> FileFilters {
        FileFilters {
            excludes: self.excludes.clone(),
        }
    }
}

#[derive(Args)]
pub struct SearchArgs {
    /// What the code does, in English or Spanish, e.g. "where do we retry a failed payment"
    pub query: Option<String>,
    /// Repository or folder to search
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Results to show
    #[arg(long, short = 'n', default_value_t = SearchSettings::TOP)]
    pub top: usize,
    /// Candidates the judge reads; more is slower and slightly more accurate
    #[arg(long, default_value_t = SearchSettings::JUDGED)]
    pub judge_top: usize,
    /// Rank by embeddings only, without the judge
    #[arg(long)]
    pub no_judge: bool,
    #[command(flatten)]
    pub filter: FilterArgs,
    /// Machine-readable output, for scripts and agents
    #[arg(long)]
    pub json: bool,
    /// Lines of code shown under each result
    #[arg(long, default_value_t = SearchSettings::PREVIEW_LINES)]
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
        let request = SearchRequest::new(query, &self.path, self.top, judge_top, self.filter.filters())?;
        let mut backend = SearchBackend::new(self.models.clone(), self.threads, !self.no_server);
        let mut display = ProgressDisplay::new();
        let mut indexed_anything = false;
        let (response, answered) = backend.search(&request, &mut |event| {
            indexed_anything |= matches!(event, crate::progress::IndexEvent::Embedding { .. });
            display.show(&event);
        })?;
        display.clear();
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
        Self::summary(
            &mut display,
            &response,
            answered,
            started.elapsed().as_secs_f64(),
            indexed_anything,
        );
        Ok(())
    }

    fn summary(
        display: &mut ProgressDisplay,
        response: &SearchResponse,
        answered: Answered,
        total: f64,
        indexed: bool,
    ) {
        let source = match answered {
            Answered::Server => "s1grep serve",
            Answered::Local => "this process",
        };
        let coverage = if response.is_complete() {
            format!("{} functions", Units::count(response.functions))
        } else {
            let percent = response.indexed * 100 / response.functions.max(1);
            display.warn(&format!(
                "searched {} of {} functions ({percent} %)",
                Units::count(response.indexed),
                Units::count(response.functions)
            ))
        };
        display.line(&display.dim(&format!(
            "{coverage} · search {:.1} s · total {:.1} s · {source}",
            response.search_seconds, total
        )));
        if !response.is_complete() {
            display
                .line(&display.dim("The rest is being indexed by another s1grep; `s1grep status` shows its progress."));
        }
        if answered == Answered::Local && (indexed || total > SearchSettings::SLOW_SEARCH_SECONDS) {
            display.line(&display.accent(
                "Tip: `s1grep serve` in another terminal keeps the models loaded: searches take about a second.",
            ));
        }
    }
}
