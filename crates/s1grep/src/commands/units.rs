use std::path::PathBuf;

use clap::Args;
use s1_index::{PythonExtractor, SourceWalker};
use serde_json::json;

#[derive(Args)]
pub struct UnitsCommand {
    /// Repository to split into functions
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Also list files under tests/ and migrations/
    #[arg(long)]
    include_tests: bool,
}

impl UnitsCommand {
    /// Prints one JSON line per function: the same units the index would store.
    pub fn run(self) -> anyhow::Result<()> {
        let mut extractor = PythonExtractor::new()?;
        for file in SourceWalker::new(&self.path, self.include_tests).files() {
            let bytes = std::fs::read(&file.absolute)?;
            for unit in extractor.extract(&file.relative, &String::from_utf8_lossy(&bytes)) {
                println!(
                    "{}",
                    json!({"path": unit.path, "name": unit.name, "start_line": unit.start_line,
                           "end_line": unit.end_line, "source": unit.source})
                );
            }
        }
        Ok(())
    }
}
