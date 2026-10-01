use clap::Args;

use crate::hub::ModelInstaller;
use crate::models::ModelDirectory;

#[derive(Args)]
pub struct SetupCommand {
    /// Download again even when the models are already there
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    models: ModelDirectory,
}

impl SetupCommand {
    /// Downloads the models (about 2.4 GB, checked with SHA-256) and says what to do next.
    pub fn run(self) -> anyhow::Result<()> {
        let root = self.models.resolved()?;
        if ModelInstaller::is_complete(&root) && !self.force {
            eprintln!("Models already in {}.", root.display());
        } else {
            eprintln!("Downloading the models into {} (about 2.4 GB, once).", root.display());
            ModelInstaller::new().install(&root, self.force)?;
            eprintln!("Models ready in {}.", root.display());
        }
        eprintln!();
        eprintln!("Next:");
        eprintln!("  s1grep \"where do we retry a failed payment\" path/to/repo   search");
        eprintln!("  s1grep status                                             what is loaded and indexed");
        eprintln!("  s1grep skill --install                                    teach Claude Code to use s1grep");
        Ok(())
    }
}
