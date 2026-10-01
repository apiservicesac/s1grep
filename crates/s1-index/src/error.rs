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
    #[error(
        "the {database} database has schema version {found}, newer than the {supported} this s1grep knows; update s1grep"
    )]
    NewerSchema {
        database: &'static str,
        found: usize,
        supported: usize,
    },
}
