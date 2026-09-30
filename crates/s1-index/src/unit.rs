/// One searchable piece of code: a top-level function or a method written directly in a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeUnit {
    /// Path relative to the indexed root, with `/` separators.
    pub path: String,
    /// `function` or `Class.method`.
    pub name: String,
    /// First and last line, 1-based and inclusive.
    pub start_line: usize,
    pub end_line: usize,
    /// The definition's source, from `def` (decorators excluded) to its last statement.
    pub source: String,
}

impl CodeUnit {
    /// Characters of source the judge reads, the same limit s1-code was trained with.
    pub const JUDGE_SOURCE_CHARACTERS: usize = 1500;

    /// Text the retrieval model embeds: path, name and the full source (the model truncates by tokens).
    pub fn document_text(&self) -> String {
        format!("{}\n{}\n\n{}", self.path, self.name, self.source)
    }

    /// Text the judge reads as its state.
    pub fn judge_state(&self) -> String {
        let source: String = self.source.chars().take(Self::JUDGE_SOURCE_CHARACTERS).collect();
        format!("{}\n{}\n\n{}", self.path, self.name, source)
    }

    /// The same unit seen under another root, e.g. with the repository folder in front of the path.
    pub fn with_path_prefix(&self, prefix: &str) -> Self {
        Self { path: format!("{prefix}/{}", self.path), ..self.clone() }
    }
}
