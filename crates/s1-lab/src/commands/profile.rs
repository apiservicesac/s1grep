use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use s1_index::{SourceWalker, VectorIndex};
use s1grep::indexer::{Indexer, Pass};
use s1grep::models::Retriever;
use s1grep::project::Project;
use s1grep::service::FileFilters;

#[derive(Args)]
pub struct ProfileCommand {
    /// Indexed project to time
    path: PathBuf,
}

impl ProfileCommand {
    /// Times the parts of a search that do not involve the models, on an index that already exists.
    pub fn run(self) -> anyhow::Result<()> {
        let project = Project::locate(&self.path)?;
        let options = FileFilters::default().walk_options()?;
        let started = Instant::now();
        let files = SourceWalker::new(&project.root, options.clone()).files()?;
        Self::line("walk", started, &format!("{} files", files.len()));
        let started = Instant::now();
        let mut store = project.open_store()?;
        Self::line("open", started, "");
        let started = Instant::now();
        let states = store.file_states()?;
        Self::line("file states", started, &format!("{} files", states.len()));
        let started = Instant::now();
        let mut indexer = Indexer {
            store: &mut store,
            retriever: Retriever::Granite,
        };
        indexer.scan(&project.root, &options, &mut |_| {})?;
        Self::line("scan", started, "");
        let whole = Pass::Whole.key(Retriever::Granite);
        let outline = Pass::Outline.key(Retriever::Granite);
        let started = Instant::now();
        store.pending_count(&whole, None)?;
        Self::line("pending", started, "");
        let started = Instant::now();
        store.coverage(&whole, None)?;
        Self::line("coverage", started, "");
        let started = Instant::now();
        let index = VectorIndex::new(store.vector_rows(&whole, None)?);
        let outlines = VectorIndex::new(store.vector_rows(&outline, Some(&whole))?);
        Self::line(
            "load vectors",
            started,
            &format!("{} whole, {} outline only", index.len(), outlines.len()),
        );
        let query = vec![0.0_f32; index.rows().first().map_or(0, |_| 768)];
        let started = Instant::now();
        index.nearest(&query, 25, |_| true);
        Self::line("rank", started, "");
        Ok(())
    }

    fn line(step: &str, started: Instant, detail: &str) {
        println!(
            "{step:<12} {:>7.0} ms  {detail}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
}
