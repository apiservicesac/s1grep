use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use s1_index::IndexStore;
use serde_json::json;

use crate::models::{CacheDirectory, ModelDirectory, Retriever};
use crate::searcher::{Hit, Indexer, Searcher};

#[derive(Args)]
pub struct SearchCommand {
    /// What the code does, in English or Spanish
    query: String,
    /// Repository to search (indexed on the fly; only changed files are re-read)
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Results to show
    #[arg(long, short = 'n', default_value_t = 5)]
    top: usize,
    /// Candidates the judge reads (default: 5)
    #[arg(long)]
    judge_top: Option<usize>,
    /// Skip the System One judge and rank by embeddings only
    #[arg(long)]
    no_judge: bool,
    #[arg(long, value_enum, default_value_t = Retriever::Granite)]
    retriever: Retriever,
    /// Machine-readable output for agents
    #[arg(long)]
    json: bool,
    /// Also search files under tests/ and migrations/
    #[arg(long)]
    include_tests: bool,
    #[arg(long)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl SearchCommand {
    const PREVIEW_LINES: usize = 6;

    pub fn run(self) -> anyhow::Result<()> {
        let started = Instant::now();
        let mut searcher = Searcher::load(&self.models, self.retriever, !self.no_judge, self.threads)?;
        let mut store = IndexStore::open(&CacheDirectory::index_for(&self.path)?)?;
        let refresh =
            Indexer { store: &mut store, embedder: &mut searcher.embedder, retriever: self.retriever }.refresh(&self.path, self.include_tests)?;
        let units = store.units_with_vectors(self.retriever.key())?;
        let judged = self.judge_top.unwrap_or(self.retriever.default_judged());
        let search_started = Instant::now();
        let hits = searcher.search(&self.query, &units, judged)?;
        let search_seconds = search_started.elapsed().as_secs_f64();
        let shown: Vec<&Hit> = hits.iter().take(self.top).collect();
        if self.json {
            let results: Vec<_> = shown.iter().enumerate().map(|(rank, hit)| hit.to_json(rank + 1)).collect();
            let document = json!({
                "query": self.query, "retriever": self.retriever.key(), "judge": searcher.has_judge(),
                "judged": if searcher.has_judge() { judged } else { 0 }, "functions": units.len(),
                "search_seconds": (search_seconds * 1000.0).round() / 1000.0, "results": results,
            });
            println!("{}", serde_json::to_string_pretty(&document)?);
            return Ok(());
        }
        for (rank, hit) in shown.iter().enumerate() {
            let judge = hit.judge.map_or(String::from("  -  "), |value| format!("{:>4.0}%", value * 100.0));
            println!(
                "{:>2}. {}:{}-{}  {}   [judge {judge} · similarity {:.2}]",
                rank + 1,
                self.path.join(&hit.unit.path).display(),
                hit.unit.start_line,
                hit.unit.end_line,
                hit.unit.name,
                hit.similarity
            );
            for line in hit.unit.source.lines().take(Self::PREVIEW_LINES) {
                println!("      {line}");
            }
            println!();
        }
        eprintln!(
            "{} functions · {} files re-indexed · search {:.2} s · total {:.2} s",
            units.len(),
            refresh.changed_files,
            search_seconds,
            started.elapsed().as_secs_f64()
        );
        Ok(())
    }
}
