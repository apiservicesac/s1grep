use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Args;
use s1_engine::ModelBundle;

/// Where the model bundle lives: `--model`, `S1GREP_MODEL_DIR`, or the user cache.
#[derive(Args, Clone)]
pub struct ModelLocator {
    /// Model bundle directory (model.onnx, tokenizer.json, decision_config.json)
    #[arg(long = "model", env = "S1GREP_MODEL_DIR", global = true)]
    directory: Option<PathBuf>,
}

impl ModelLocator {
    const DEFAULT_MODEL: &'static str = "laya-multilingual";

    pub fn open(&self) -> anyhow::Result<ModelBundle> {
        let directory = match &self.directory {
            Some(directory) => directory.clone(),
            None => Self::cache_directory()?.join("models").join(Self::DEFAULT_MODEL),
        };
        if !directory.is_dir() {
            bail!("no model at {} (pass --model or set S1GREP_MODEL_DIR)", directory.display());
        }
        ModelBundle::open(&directory).with_context(|| format!("opening model bundle {}", directory.display()))
    }

    fn cache_directory() -> anyhow::Result<PathBuf> {
        if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(cache).join("s1grep"));
        }
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home).join(".cache").join("s1grep"))
    }
}
