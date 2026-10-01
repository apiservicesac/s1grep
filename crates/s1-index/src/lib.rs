//! Code units, their embeddings and the on-disk index s1grep searches.

mod error;
mod extractor;
mod fingerprint;
mod languages;
mod lexical;
mod ranking;
mod schema;
mod settings;
mod space;
mod store;
mod unit;
mod vector_index;
mod walker;

pub use error::IndexError;
pub use extractor::PythonExtractor;
pub use fingerprint::ContentFingerprint;
pub use languages::{Container, ExtractorRegistry, LanguageSpec};
pub use lexical::{CodeTokenizer, LexicalHit, LexicalIndex};
pub use ranking::{FusionWeights, RankFusion};
pub use schema::SchemaMigrations;
pub use settings::{FusionTable, IndexLimits, LanguageSettings, LexicalSettings};
pub use space::EmbeddingSpace;
pub use store::{Coverage, FileState, IndexStore, StoredUnit, VectorCache};
pub use unit::CodeUnit;
pub use vector_index::{VectorIndex, VectorRow};
pub use walker::{PathExcludes, SourceFile, SourceWalker, WalkOptions};
