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

    /// Name stored next to the vectors of function outlines, the quick first pass over a large project.
    pub fn outline_key(self) -> &'static str {
        match self {
            Self::Granite => "granite-outline",
            Self::Qwen3 => "qwen3-outline",
        }
    }

    /// Candidates the judge reads by default.
    pub fn default_judged(self) -> usize {
        crate::settings::SearchSettings::JUDGED
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
    /// The folder given with `--models` or `S1GREP_MODELS`, if any.
    pub fn explicit(&self) -> Option<&std::path::Path> {
        self.folder.as_deref()
    }

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

    /// One index per project, named after a hash of its absolute path, so indexing never writes into the project.
    pub fn project_index(root: &std::path::Path) -> anyhow::Result<PathBuf> {
        let key = s1_index::content_hash(root.to_string_lossy().as_bytes());
        Ok(Self::root()?.join("projects").join(format!("{}.sqlite", &key[..16])))
    }

    /// Vectors shared by every project, keyed by the content they were computed from.
    pub fn vectors() -> anyhow::Result<PathBuf> {
        Ok(Self::root()?.join("vectors.sqlite"))
    }

    /// Every project index in the cache.
    pub fn project_indexes() -> anyhow::Result<Vec<PathBuf>> {
        let folder = Self::root()?.join("projects");
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return Ok(Vec::new());
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "sqlite"))
            .collect();
        paths.sort();
        Ok(paths)
    }
}
