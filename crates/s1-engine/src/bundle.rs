use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::EngineError;

/// The fields of Laya's `rl_agent_config.json` that inference needs.
#[derive(Debug, Clone, Deserialize)]
pub struct DecisionConfig {
    #[serde(default = "DecisionConfig::default_max_len")]
    pub max_len: usize,
    #[serde(default = "DecisionConfig::default_head_max_len")]
    pub head_max_len: usize,
    #[serde(default)]
    pub temperature: Vec<serde_json::Value>,
    #[serde(default)]
    pub temperature_by_options: HashMap<String, serde_json::Value>,
}

impl DecisionConfig {
    fn default_max_len() -> usize {
        512
    }

    fn default_head_max_len() -> usize {
        192
    }
}

/// A model directory as produced by `model_export export`: the ONNX graph, the tokenizer and the
/// decision config, with nothing else needed at runtime.
#[derive(Debug, Clone)]
pub struct ModelBundle {
    pub directory: PathBuf,
    pub config: DecisionConfig,
}

impl ModelBundle {
    pub const GRAPH_FILE: &'static str = "model.onnx";
    pub const TOKENIZER_FILE: &'static str = "tokenizer.json";
    pub const TOKENIZER_CONFIG_FILE: &'static str = "tokenizer_config.json";
    pub const DECISION_CONFIG_FILE: &'static str = "decision_config.json";

    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, EngineError> {
        let directory = directory.into();
        for file in [Self::GRAPH_FILE, Self::TOKENIZER_FILE, Self::TOKENIZER_CONFIG_FILE, Self::DECISION_CONFIG_FILE] {
            let path = directory.join(file);
            if !path.is_file() {
                return Err(EngineError::MissingFile(path));
            }
        }
        let config_path = directory.join(Self::DECISION_CONFIG_FILE);
        let text = std::fs::read_to_string(&config_path).map_err(|source| EngineError::Io { path: config_path.clone(), source })?;
        let config = serde_json::from_str(&text).map_err(|source| EngineError::InvalidJson { path: config_path, source })?;
        Ok(Self { directory, config })
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    pub fn graph_path(&self) -> PathBuf {
        self.file(Self::GRAPH_FILE)
    }

    pub fn tokenizer_path(&self) -> PathBuf {
        self.file(Self::TOKENIZER_FILE)
    }

    pub fn tokenizer_config_path(&self) -> PathBuf {
        self.file(Self::TOKENIZER_CONFIG_FILE)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}
