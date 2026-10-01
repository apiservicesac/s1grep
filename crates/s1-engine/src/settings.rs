//! The engine's fixed values in one place. Most mirror the Python reference (Laya) and must change only together with
//! it: the parity tests compare the two token for token.

/// Files of a model bundle on disk.
pub struct BundleFiles;

impl BundleFiles {
    pub const GRAPH: &'static str = "model.onnx";
    pub const TOKENIZER: &'static str = "tokenizer.json";
    pub const TOKENIZER_CONFIG: &'static str = "tokenizer_config.json";
    /// Decision bundles only: sequence limits and calibration temperatures.
    pub const DECISION_CONFIG: &'static str = "decision_config.json";
    /// Embedder bundles only: pooling, normalisation and prompts.
    pub const EMBEDDER_CONFIG: &'static str = "embedder_config.json";
}

/// How Laya fits a question and its options into a sequence (`laya.common.build_sequence`).
pub struct SequenceLimits;

impl SequenceLimits {
    /// Tokens kept from each option's text.
    pub const OPTION_TOKENS: usize = 48;
    /// Below this many tokens left for the instructions, options are cut to share the head budget.
    pub const MINIMUM_HEAD_BUDGET: usize = 16;
    pub const MINIMUM_OPTION_TOKENS: usize = 4;
    pub const MINIMUM_INSTRUCTION_TOKENS: usize = 8;
    /// Sequence and head lengths of a decision bundle whose config names none.
    pub const DEFAULT_MAX_LEN: usize = 512;
    pub const DEFAULT_HEAD_MAX_LEN: usize = 192;
}

/// Bounds of the calibration temperatures read from a bundle.
pub struct CalibrationLimits;

impl CalibrationLimits {
    pub const MINIMUM_TEMPERATURE: f64 = 0.5;
    pub const MAXIMUM_TEMPERATURE: f64 = 5.0;
}

/// Option texts of a noul (yes/no) question when the caller gives none, as Laya renders them.
pub struct NoulDefaults;

impl NoulDefaults {
    pub const FALSE: &'static str = "no, the statement does not hold";
    pub const TRUE: &'static str = "yes, the statement holds";
}

/// Sentence embedders.
pub struct EmbedderSettings;

impl EmbedderSettings {
    /// Texts per ONNX Runtime run; sorted by length so each batch pads little.
    pub const BATCH: usize = 16;
    /// Tokens tried, in order, as padding when the bundle names none.
    pub const PAD_CANDIDATES: [&'static str; 4] = ["<pad>", "[PAD]", "<|endoftext|>", "</s>"];
    /// Smallest norm divided by when normalising a vector, so an all-zero output does not divide by zero.
    pub const MINIMUM_NORM: f32 = 1e-12;
}
