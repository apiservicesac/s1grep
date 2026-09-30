use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("model file not found: {0}")]
    MissingFile(PathBuf),
    #[error("could not read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid JSON in {path}: {source}")]
    InvalidJson { path: PathBuf, source: serde_json::Error },
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
    #[error("special token {0:?} is not in the tokenizer vocabulary")]
    MissingSpecialToken(String),
    #[error("invalid question {id:?}: {reason}")]
    InvalidQuestion { id: String, reason: String },
    #[error("question {id:?} has {expected} options but only {fitted} fit in head_max_len={head_max_len}")]
    OptionsOverflow { id: String, expected: usize, fitted: usize, head_max_len: usize },
    #[error("ONNX Runtime error: {0}")]
    Runtime(#[from] ort::Error),
    #[error("device {0:?} is not available in this build")]
    UnsupportedAccelerator(&'static str),
    #[error("unexpected model output: {0}")]
    UnexpectedOutput(String),
}
