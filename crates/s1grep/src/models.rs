use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, ValueEnum};

/// Which embedding model finds the candidates.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Retriever {
    /// granite-embedding-278m-multilingual: fast to index on a CPU
    Granite,
    /// Qwen3-Embedding-0.6B: better candidates, about 6 times slower to index (not shipped yet)
    #[value(hide = true)]
    Qwen3,
}

impl Retriever {
    pub fn bundle_name(self) -> &'static str {
        match self {
            Self::Granite => "granite-278m-onnx",
            Self::Qwen3 => "qwen3-embedding-0.6b-onnx",
        }
    }

    /// Name stored next to each vector in the index, so both retrievers can share one index.
    pub fn key(self) -> &'static str {
        match self {
            Self::Granite => "granite",
            Self::Qwen3 => "qwen3",
        }
    }

    /// Candidates the judge reads by default: 5 costs half of 10 on a CPU for 4 fewer right answers in 100 on dev.
    pub fn default_judged(self) -> usize {
        match self {
            Self::Granite => 5,
            Self::Qwen3 => 5,
        }
    }
}

/// Where the model bundles live: `--models`, `S1GREP_MODELS`, or `~/.cache/s1grep/models`.
#[derive(Args, Clone)]
pub struct ModelDirectory {
    /// Folder holding the model bundles (s1-code-v3-onnx, granite-278m-onnx, ...)
    #[arg(id = "models", long = "models", env = "S1GREP_MODELS", value_name = "FOLDER")]
    folder: Option<PathBuf>,
}

impl ModelDirectory {
    pub const JUDGE_BUNDLE: &'static str = "s1-code-v3-onnx";

    /// The folder in use: `--models`, `S1GREP_MODELS`, or the cache.
    pub fn resolved(&self) -> anyhow::Result<PathBuf> {
        match &self.folder {
            Some(folder) => Ok(folder.clone()),
            None => Ok(CacheDirectory::root()?.join("models")),
        }
    }

    pub fn bundle(&self, name: &str) -> anyhow::Result<PathBuf> {
        let root = self.resolved()?;
        let directory = root.join(name);
        if !directory.is_dir() {
            bail!(
                "model {name} is not in {}; run `s1grep setup` to download the models",
                root.display()
            );
        }
        Ok(directory)
    }
}

/// The per-user cache: `$XDG_CACHE_HOME/s1grep`, or `~/.cache/s1grep` (`%LOCALAPPDATA%\s1grep` on Windows).
pub struct CacheDirectory;

impl CacheDirectory {
    pub fn root() -> anyhow::Result<PathBuf> {
        if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(cache).join("s1grep"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local).join("s1grep"));
        }
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home).join(".cache").join("s1grep"))
    }

    /// One index per repository, named after a hash of its absolute path, so indexing never writes into the repo.
    pub fn index_for(repository: &std::path::Path) -> anyhow::Result<PathBuf> {
        let absolute =
            std::fs::canonicalize(repository).with_context(|| format!("resolving {}", repository.display()))?;
        let key = s1_index::content_hash(absolute.to_string_lossy().as_bytes());
        Ok(Self::root()?.join("indexes").join(format!("{}.sqlite", &key[..16])))
    }
}
