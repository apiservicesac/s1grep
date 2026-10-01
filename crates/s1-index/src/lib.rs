//! Code units, their embeddings and the on-disk index s1grep searches.

mod error;
mod extractor;
mod fingerprint;
mod ranking;
mod settings;
mod store;
mod unit;
mod walker;

pub use error::IndexError;
pub use extractor::PythonExtractor;
pub use fingerprint::ContentFingerprint;
pub use ranking::{FusionWeights, RankFusion, VectorRanking};
pub use settings::{FusionTable, IndexLimits};
pub use store::{Coverage, FileState, IndexStore, SearchableUnit, StoredUnit};
pub use unit::CodeUnit;
pub use walker::{SourceFile, SourceWalker, WalkOptions};
