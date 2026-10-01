use clap::Args;
use s1_index::IndexStore;

use crate::cache::{CacheDirectory, ProjectFolder};
use crate::hub::ModelInstaller;
use crate::indexer::Pass;
use crate::models::{ModelDirectory, Retriever};
use crate::progress::{ProgressDisplay, Units};
use crate::project::IndexLock;
use crate::server::ServerClient;
use crate::settings::DisplaySettings;

#[derive(Args)]
pub struct StatusCommand {
    #[command(flatten)]
    models: ModelDirectory,
}

impl StatusCommand {
    /// One screen with the server, the models and every indexed project.
    pub fn run(self) -> anyhow::Result<()> {
        let display = ProgressDisplay::new();
        let busy = ServerClient::any().is_none() && ServerClient::recorded().is_some();
        let server = match ServerClient::recorded() {
            Some(client) => {
                let lifetime = match client.info.idle_minutes {
                    Some(minutes) => format!("stops after {minutes} min without searches"),
                    None => "started by hand".to_string(),
                };
                format!(
                    "{}  {}",
                    if busy {
                        display.warn(&format!(
                            "● busy indexing (pid {}), searches wait for it",
                            client.info.pid
                        ))
                    } else {
                        display.good(&format!("● running (pid {}), models in memory", client.info.pid))
                    },
                    display.dim(&format!("{lifetime} · `s1grep stop` stops it now"))
                )
            }
            None => display.dim("○ not running · the next search starts it"),
        };
        let models_root = self.models.resolved()?;
        let models = if ModelInstaller::is_complete(&models_root) {
            display.good("● ready")
        } else {
            display.warn("○ missing · run `s1grep setup`")
        };
        println!("Process   {server}");
        println!(
            "Models    {models}  {}",
            display.dim(&models_root.display().to_string())
        );
        println!();
        let projects = Self::projects()?;
        if projects.is_empty() {
            println!(
                "{}",
                display.dim("No project indexed yet: the first search in a folder indexes it.")
            );
            return Ok(());
        }
        let width = projects
            .iter()
            .map(|project| project.root.len())
            .max()
            .unwrap_or(7)
            .clamp(7, 60);
        println!("{:<width$}  {:>9}  Index", "Project", "Functions");
        for project in projects {
            let state = if project.embedded >= project.functions {
                display.good("● complete")
            } else {
                let percent = project.embedded * 100 / project.functions.max(1);
                let filled = percent * DisplaySettings::STATUS_BAR_WIDTH / 100;
                let activity = if project.indexing {
                    "indexing now"
                } else {
                    "paused · the next search continues"
                };
                format!(
                    "{}{} {percent:>3} %  {}",
                    display.accent(&"━".repeat(filled)),
                    display.dim(&"━".repeat(DisplaySettings::STATUS_BAR_WIDTH - filled)),
                    display.dim(activity)
                )
            };
            println!(
                "{:<width$}  {:>9}  {state}",
                Self::shorten(&project.root, width),
                Units::count(project.functions)
            );
        }
        Ok(())
    }

    fn projects() -> anyhow::Result<Vec<ProjectState>> {
        let whole = Pass::Whole.key(Retriever::Granite);
        let mut projects = Vec::new();
        for folder in ProjectFolder::all()? {
            let Some(info) = folder.info() else { continue };
            let Ok(store) = IndexStore::open(&folder.catalog(), &CacheDirectory::vectors()?) else {
                continue;
            };
            let coverage = store.coverage(&whole, None)?;
            projects.push(ProjectState {
                root: info.root,
                functions: coverage.units,
                embedded: coverage.embedded,
                indexing: IndexLock::is_held(&folder),
            });
        }
        projects.sort_by(|left, right| left.root.cmp(&right.root));
        Ok(projects)
    }

    fn shorten(path: &str, width: usize) -> String {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = match path.strip_prefix(&home) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => path.to_string(),
        };
        if path.chars().count() <= width {
            return path;
        }
        let tail: String = path
            .chars()
            .rev()
            .take(width - 1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

struct ProjectState {
    root: String,
    functions: usize,
    embedded: usize,
    indexing: bool,
}
