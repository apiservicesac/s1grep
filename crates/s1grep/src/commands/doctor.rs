use std::path::PathBuf;

use clap::Args;

use crate::hub::Published;
use crate::models::{CacheDirectory, ModelDirectory, Retriever};
use crate::progress::Units;
use crate::project::Project;
use crate::server::ServerClient;

#[derive(Args)]
pub struct DoctorCommand {
    /// Repository whose index to report
    #[arg(default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    models: ModelDirectory,
}

impl DoctorCommand {
    /// Reports what s1grep needs and what it found, without loading the models.
    pub fn run(self) -> anyhow::Result<()> {
        let mut ready = true;
        println!("s1grep {}", env!("CARGO_PKG_VERSION"));
        let root = self.models.resolved()?;
        println!("\nModels  {}", root.display());
        for published in &Published::ALL {
            let present = root.join(published.bundle).join("model.onnx").is_file();
            ready &= present;
            println!("  {}  {}", Self::mark(present), published.bundle);
        }
        if !ready {
            println!("     run `s1grep setup` to download them");
        }
        println!("\nServer");
        match ServerClient::connect() {
            Some(client) => println!(
                "  {}  s1grep serve on 127.0.0.1:{} (pid {})",
                Self::mark(true),
                client.info.port,
                client.info.pid
            ),
            None => println!("  -  not running; searches load the models each time (`s1grep serve` avoids that)"),
        }
        println!("\nIndex");
        match Project::locate(&self.path) {
            Ok(project) => {
                if CacheDirectory::project_index(&project.root)?.is_file() {
                    let coverage = project
                        .open_store()?
                        .coverage(Retriever::Granite.key(), project.scope.as_deref())?;
                    println!(
                        "  {}  {} of {} functions indexed in {}",
                        Self::mark(coverage.is_complete()),
                        Units::count(coverage.embedded),
                        Units::count(coverage.units),
                        project.target().display()
                    );
                } else {
                    println!(
                        "  -  {} is not indexed yet; the first search indexes it",
                        project.target().display()
                    );
                }
            }
            Err(_) => println!("  {}  {} does not exist", Self::mark(false), self.path.display()),
        }
        println!("\n{}", if ready { "Ready." } else { "Not ready: see above." });
        Ok(())
    }

    fn mark(good: bool) -> &'static str {
        if good { "ok" } else { "!!" }
    }
}
