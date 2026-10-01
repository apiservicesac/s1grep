//! The index's fixed values in one place.

use std::time::Duration;

/// What becomes a searchable code unit and how it is stored.
pub struct IndexLimits;

impl IndexLimits {
    /// Characters of source the judge reads: the limit s1-code was trained with, so it changes only with the model.
    pub const JUDGE_SOURCE_CHARACTERS: usize = 1500;
    /// Shorter definitions (one-line getters, `pass` stubs) are not worth a search result.
    pub const MINIMUM_LINES: usize = 3;
    /// Hex characters kept from the blake3 hash that identifies a unit's content.
    pub const CONTENT_KEY_LENGTH: usize = 32;
    /// How long a write waits for another process holding the index database.
    pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);
}
