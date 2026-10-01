use std::path::PathBuf;

use clap::Args;
use s1_index::IndexStore;

use crate::models::{CacheDirectory, ModelDirectory, Retriever};
use crate::searcher::{Indexer, Searcher};

#[derive(Args)]
pub struct IndexCommand {
    /// Repository to index
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Embedding model that finds candidates
    #[arg(long, value_enum, default_value_t = Retriever::Granite)]
    retriever: Retriever,
    /// Also index files under tests/ and migrations/
    #[arg(long)]
    include_tests: bool,
    #[arg(long)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl IndexCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let mut searcher = Searcher::load(&self.models, self.retriever, false, self.threads)?;
        let index_path = CacheDirectory::index_for(&self.path)?;
        let mut store = IndexStore::open(&index_path)?;
        let report = Indexer {
            store: &mut store,
            embedder: &mut searcher.embedder,
            retriever: self.retriever,
        }
        .refresh(&self.path, self.include_tests)?;
        eprintln!(
            "{} files ({} changed, {} removed), {} functions, {} embedded with {} in {:.1} s -> {}",
            report.files,
            report.changed_files,
            report.removed_files,
            report.units,
            report.embedded_units,
            self.retriever.key(),
            report.seconds,
            index_path.display()
        );
        Ok(())
    }
}
