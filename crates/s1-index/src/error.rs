use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("could not read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("index database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("parser error: {0}")]
    Parser(String),
    #[error("invalid exclude pattern {0}")]
    Pattern(String),
}
