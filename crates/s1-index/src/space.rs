use crate::settings::IndexLimits;

/// Which vectors can be compared with each other: the same model, at the same revision and dimension, embedding the
/// same kind of text. Every stored vector is keyed by its space, so a change to any of these never mixes old vectors
/// with new ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSpace {
    pub model: &'static str,
    pub revision: &'static str,
    pub dimension: usize,
    /// The text that is embedded and its format version, e.g. `whole-v1`.
    pub text_format: &'static str,
}

impl EmbeddingSpace {
    pub fn key(&self) -> String {
        let revision: String = self.revision.chars().take(IndexLimits::SPACE_REVISION_LENGTH).collect();
        format!("{}@{revision}/{}d/{}", self.model, self.dimension, self.text_format)
    }
}

#[cfg(test)]
mod tests {
    use super::EmbeddingSpace;

    #[test]
    fn the_key_names_model_revision_size_and_text() {
        let space = EmbeddingSpace {
            model: "granite",
            revision: "b795cbc00b23bcaafbbbba6b242448104cc62ec0",
            dimension: 768,
            text_format: "whole-v1",
        };
        assert_eq!(space.key(), "granite@b795cbc00b23/768d/whole-v1");
    }
}
