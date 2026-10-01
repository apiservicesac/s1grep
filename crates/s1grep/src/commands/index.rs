use std::path::PathBuf;
use std::time::Instant;

use clap::Args;

use crate::models::ModelDirectory;
use crate::progress::{IndexEvent, ProgressDisplay, Units};
use crate::project::MissingFolder;
use crate::server::BackgroundServer;
use crate::service::ProjectProgress;
use crate::settings::ServerSettings;

#[derive(Args)]
pub struct IndexCommand {
    /// Project or folder to index
    #[arg(default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    models: ModelDirectory,
}

impl IndexCommand {
    /// Hands the project to the background process and follows its indexing to the end. Stopping this command
    /// (Ctrl+C) only stops following: the indexing goes on.
    pub fn run(self) -> anyhow::Result<()> {
        let started = Instant::now();
        let target = MissingFolder::resolve(&self.path)?;
        let mut display = ProgressDisplay::new();
        let client = BackgroundServer::ensure(&self.models, &mut |event| display.show(&event))?;
        let mut progress = client.index(&target, &mut |event| display.show(&event))?;
        if !progress.is_complete() {
            display
                .line(&display.dim("Indexing in the background process; Ctrl+C stops following it, not the indexing."));
        }
        while !progress.is_complete() {
            if progress.indexing.is_none() && !progress.held_elsewhere {
                // Nothing is indexing it any more (the job failed or the process restarted): ask again.
                progress = client.index(&target, &mut |event| display.show(&event))?;
                if progress.indexing.is_none() {
                    break;
                }
            }
            display.show(&Self::event(&progress));
            std::thread::sleep(ServerSettings::FOLLOW_INTERVAL);
            progress = client.progress(&target)?;
        }
        display.clear();
        let summary = format!(
            "{} {} functions in {} · {}",
            if progress.is_complete() {
                "Indexed"
            } else {
                "Indexing stopped at"
            },
            Units::count(progress.indexed),
            progress.root.display(),
            Units::duration(started.elapsed().as_secs_f64())
        );
        if progress.is_complete() {
            display.line(&display.good(&summary));
        } else {
            display.line(&display.warn(&summary));
        }
        Ok(())
    }

    /// The bar to show: functions made searchable first, then functions indexed in full.
    fn event(progress: &ProjectProgress) -> IndexEvent {
        let searchable = progress.searchable < progress.functions;
        let indexing = progress.indexing.as_ref();
        IndexEvent::Background {
            searchable,
            done: if searchable {
                progress.searchable
            } else {
                progress.indexed
            },
            total: progress.functions,
            seconds_left: indexing.and_then(|indexing| {
                if searchable {
                    indexing.searchable_seconds_left
                } else {
                    indexing.seconds_left
                }
            }),
        }
    }
}
