use std::path::PathBuf;
use std::time::Instant;

use clap::Args;

use crate::indexer::Pass;
use crate::models::ModelDirectory;
use crate::progress::{IndexEvent, ProgressDisplay, Units};
use crate::service::SearchService;

#[derive(Args)]
pub struct IndexCommand {
    /// Project or folder to index
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long, hide = true)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl IndexCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let started = Instant::now();
        let mut display = ProgressDisplay::new();
        display.show(&IndexEvent::LoadingModels);
        let mut service = SearchService::load(&self.models, false, self.threads)?;
        let project = service.index(&self.path, &mut |event| display.show(&event))?;
        let coverage = project
            .open_store()?
            .coverage(&Pass::Whole.key(service.retriever()), project.scope.as_deref())?;
        display.line(&display.good(&format!(
            "Indexed {} functions in {} · {}",
            Units::count(coverage.units),
            project.target().display(),
            Units::duration(started.elapsed().as_secs_f64())
        )));
        Ok(())
    }
}
