use std::collections::HashSet;
use std::path::Path;

use clap::Args;
use s1_index::{IndexStore, VectorCache};

use crate::cache::{CacheDirectory, ProjectFolder};
use crate::indexer::Pass;
use crate::models::Retriever;
use crate::progress::{ProgressDisplay, Units};
use crate::project::IndexLock;

#[derive(Args)]
pub struct GcCommand {
    /// Show what would be removed without removing anything
    #[arg(long)]
    dry_run: bool,
}

impl GcCommand {
    /// Removes indexes of folders that no longer exist and vectors that no index uses any more.
    pub fn run(self) -> anyhow::Result<()> {
        let display = ProgressDisplay::new();
        let mut kept = Vec::new();
        let mut removed_projects = 0;
        for folder in ProjectFolder::all()? {
            let Some(info) = folder.info() else {
                kept.push(folder);
                continue;
            };
            if Path::new(&info.root).is_dir() {
                kept.push(folder);
                continue;
            }
            println!(
                "{} {}",
                if self.dry_run { "Would remove" } else { "Removed" },
                info.root
            );
            if !self.dry_run {
                // The lock proves nobody indexes it; it is released before the folder holding it is deleted.
                match IndexLock::acquire(&folder)? {
                    Ok(lock) => drop(lock),
                    Err(_) => {
                        kept.push(folder);
                        continue;
                    }
                }
                folder.remove()?;
            }
            removed_projects += 1;
        }
        if kept.iter().any(IndexLock::is_held) {
            println!(
                "{}",
                display.warn("A project is being indexed right now; vectors are left as they are. Run gc again later.")
            );
            return self.summary(&display, removed_projects, None);
        }
        let mut contents = HashSet::new();
        for folder in &kept {
            let store = IndexStore::open(&folder.catalog(), &CacheDirectory::vectors()?)?;
            contents.extend(store.content_keys()?);
        }
        let spaces = [Pass::Whole, Pass::Outline].map(|pass| pass.key(Retriever::Granite));
        let removed_vectors = if self.dry_run {
            None
        } else {
            Some(VectorCache::open(&CacheDirectory::vectors()?)?.remove_unused(&spaces, &contents)?)
        };
        self.summary(&display, removed_projects, removed_vectors)
    }

    fn summary(&self, display: &ProgressDisplay, projects: usize, vectors: Option<usize>) -> anyhow::Result<()> {
        let size = std::fs::metadata(CacheDirectory::vectors()?).map_or(0, |metadata| metadata.len());
        let vectors = vectors.map_or(String::new(), |count| {
            format!(", {} unused vectors removed", Units::count(count))
        });
        println!(
            "{}",
            display.good(&format!(
                "{} {} indexes of missing folders{vectors} · vector cache {} MB",
                if self.dry_run { "Would remove" } else { "Removed" },
                Units::count(projects),
                size / 1_000_000
            ))
        );
        Ok(())
    }
}
