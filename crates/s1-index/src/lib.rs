//! Code units, their embeddings and the on-disk index s1grep searches.

mod error;
mod extractor;
mod ranking;
mod store;
mod unit;
mod walker;

pub use error::IndexError;
pub use extractor::PythonExtractor;
pub use ranking::{FusionWeights, RankFusion, VectorRanking};
pub use store::{Coverage, FileState, IndexStore, StoredUnit};
pub use unit::CodeUnit;
pub use walker::{SourceFile, SourceWalker, WalkOptions};

/// Content hash used to notice changed files.
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
