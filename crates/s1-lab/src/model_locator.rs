use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Args;
use s1_engine::ModelBundle;
use s1grep::models::CacheDirectory;

use crate::settings::BenchSettings;

/// Where the model bundle lives: `--model`, `S1GREP_MODEL_DIR`, or the user cache.
#[derive(Args, Clone)]
pub struct ModelLocator {
    /// Model bundle directory (model.onnx, tokenizer.json, decision_config.json)
    #[arg(long = "model", env = "S1GREP_MODEL_DIR", global = true)]
    directory: Option<PathBuf>,
}

impl ModelLocator {
    pub fn open(&self) -> anyhow::Result<ModelBundle> {
        let directory = match &self.directory {
            Some(directory) => directory.clone(),
            None => CacheDirectory::root()?.join("models").join(BenchSettings::MODEL),
        };
        if !directory.is_dir() {
            bail!(
                "no model at {} (pass --model or set S1GREP_MODEL_DIR)",
                directory.display()
            );
        }
        ModelBundle::open(&directory).with_context(|| format!("opening model bundle {}", directory.display()))
    }
}
