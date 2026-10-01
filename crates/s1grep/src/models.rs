use std::path::PathBuf;

use anyhow::bail;
use clap::{Args, ValueEnum};

use s1_index::EmbeddingSpace;

use crate::cache::CacheDirectory;
use crate::settings::ModelSettings;

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
            Self::Granite => ModelSettings::RETRIEVER_BUNDLE,
            Self::Qwen3 => "qwen3-embedding-0.6b-onnx",
        }
    }

    /// Short name, for the fusion weights tuned per retriever.
    pub fn key(self) -> &'static str {
        match self {
            Self::Granite => "granite",
            Self::Qwen3 => "qwen3",
        }
    }

    /// The space of this retriever's vectors of one kind of text.
    pub fn space(self, text_format: &'static str) -> EmbeddingSpace {
        match self {
            Self::Granite => EmbeddingSpace {
                model: ModelSettings::RETRIEVER_MODEL,
                revision: ModelSettings::RETRIEVER_REVISION,
                dimension: ModelSettings::RETRIEVER_DIMENSION,
                text_format,
            },
            Self::Qwen3 => EmbeddingSpace {
                model: "qwen3-embedding-0.6b",
                revision: "unpinned",
                dimension: 1024,
                text_format,
            },
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
            None => CacheDirectory::models(),
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
