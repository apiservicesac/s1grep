//! The lab's fixed values in one place.

/// The `bench` and `decide` commands.
pub struct BenchSettings;

impl BenchSettings {
    /// Bundle they load when no `--model` is given.
    pub const MODEL: &'static str = "laya-multilingual";
    /// The batched measurement: this many fragments of this many tokens.
    pub const BATCH_SIZE: usize = 8;
    pub const BATCH_TOKENS: usize = 128;
}
