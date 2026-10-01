//! The index's fixed values in one place.

use std::time::Duration;

/// What becomes a searchable code unit and how it is stored.
pub struct IndexLimits;

impl IndexLimits {
    /// Characters of source the judge reads: the limit s1-code was trained with, so it changes only with the model.
    pub const JUDGE_SOURCE_CHARACTERS: usize = 1500;
    /// Shorter definitions (one-line getters, `pass` stubs) are not worth a search result.
    pub const MINIMUM_LINES: usize = 3;
    /// The outline of a unit, embedded first so a large project is searchable in a minute: its first lines (the
    /// signature and the start of the docstring), at most this many characters.
    pub const OUTLINE_LINES: usize = 2;
    pub const OUTLINE_CHARACTERS: usize = 160;
    /// Versions of the texts the retriever embeds (`CodeUnit::document_text` and `outline_text`, with the outline
    /// limits above). Change one whenever its text changes: vectors of the old text then stop being used.
    pub const WHOLE_TEXT_FORMAT: &'static str = "whole-v1";
    pub const OUTLINE_TEXT_FORMAT: &'static str = "outline-v1";
    /// Characters of a model revision kept in an embedding space key.
    pub const SPACE_REVISION_LENGTH: usize = 12;
    /// Hex characters kept from the blake3 hash that identifies a unit's content.
    pub const CONTENT_KEY_LENGTH: usize = 32;
    /// How long a write waits for another process holding the index database.
    pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);
}

/// Fusion of the retriever's order with the judge's: (judge share, rank smoothing), chosen on the dev split of the exam
/// for s1-code v3, per retriever and number of candidates judged.
pub struct FusionTable;

impl FusionTable {
    pub const QWEN3_UP_TO_FIVE: (f64, f64) = (0.45, 5.0);
    pub const QWEN3_MORE: (f64, f64) = (0.35, 30.0);
    pub const UP_TO_FIVE: (f64, f64) = (0.55, 5.0);
    pub const UP_TO_TEN: (f64, f64) = (0.65, 1.0);
    pub const MORE: (f64, f64) = (0.75, 10.0);
}
