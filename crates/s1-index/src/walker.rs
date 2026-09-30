use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// A source file found under the indexed root.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub absolute: PathBuf,
    /// Relative to the root, with `/` separators.
    pub relative: String,
}

/// Lists the Python files of a repository the way ripgrep would: `.gitignore` respected, hidden folders and symbolic
/// links skipped (a link to `/` must never send the walk through the whole disk). Tests and migrations are left
/// out unless asked for, the same rule used when the exams were built.
pub struct SourceWalker {
    root: PathBuf,
    include_tests: bool,
}

impl SourceWalker {
    const MAXIMUM_FILE_BYTES: u64 = 1_000_000;
    const EXCLUDED_FOLDERS: [&'static str; 2] = ["tests", "migrations"];

    pub fn new(root: impl Into<PathBuf>, include_tests: bool) -> Self {
        Self { root: root.into(), include_tests }
    }

    pub fn files(&self) -> Vec<SourceFile> {
        let mut files: Vec<SourceFile> = WalkBuilder::new(&self.root)
            .follow_links(false)
            .hidden(true)
            .git_ignore(true)
            .require_git(false)
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "py"))
            .filter(|entry| entry.metadata().is_ok_and(|metadata| metadata.len() <= Self::MAXIMUM_FILE_BYTES))
            .filter_map(|entry| self.relative(entry.path()).map(|relative| (entry.path().to_path_buf(), relative)))
            .filter(|(_, relative)| self.include_tests || !Self::is_excluded(relative))
            .map(|(absolute, relative)| SourceFile { absolute, relative })
            .collect();
        files.sort_by(|left, right| left.relative.cmp(&right.relative));
        files
    }

    fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        Some(relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }

    fn is_excluded(relative: &str) -> bool {
        relative.split('/').rev().skip(1).any(|folder| Self::EXCLUDED_FOLDERS.contains(&folder))
    }
}
