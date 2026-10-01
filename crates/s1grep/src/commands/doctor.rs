use std::path::PathBuf;

use clap::Args;
use s1_index::IndexStore;

use crate::hub::Published;
use crate::models::{CacheDirectory, ModelDirectory};
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
        match std::fs::canonicalize(&self.path) {
            Ok(repository) => {
                let index = CacheDirectory::index_for(&repository)?;
                if index.is_file() {
                    let functions = IndexStore::open(&index)?.unit_count()?;
                    println!(
                        "  {}  {} functions for {}",
                        Self::mark(true),
                        functions,
                        repository.display()
                    );
                } else {
                    println!(
                        "  -  {} is not indexed yet; the first search indexes it",
                        repository.display()
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
