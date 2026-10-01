use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

use crate::error::IndexError;

/// A source file found under the indexed root.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub absolute: PathBuf,
    /// Relative to the root, with `/` separators.
    pub relative: String,
}

/// Which files the walk keeps. Rules come from files, not code: a global ignore file, the per-folder ignore files
/// found while walking, and one-off patterns.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Ignore rules for every project (gitignore syntax), lowest precedence.
    pub global_ignore: Option<PathBuf>,
    /// Name of the per-folder ignore file, e.g. `.s1grepignore`.
    pub folder_ignore_name: String,
    /// One-off gitignore-style patterns, relative to the root, highest precedence.
    pub excludes: Vec<String>,
    /// Larger files are generated or data, not code worth searching.
    pub maximum_file_bytes: u64,
    /// File extensions to read, without the dot.
    pub extensions: Vec<String>,
}

/// Lists source files the way ripgrep would: `.gitignore` and the ignore files respected, hidden folders and
/// symbolic links skipped (a link to `/` must never send the walk through the whole disk).
pub struct SourceWalker {
    root: PathBuf,
    options: WalkOptions,
}

impl SourceWalker {
    pub fn new(root: impl Into<PathBuf>, options: WalkOptions) -> Self {
        Self {
            root: root.into(),
            options,
        }
    }

    pub fn files(&self) -> Result<Vec<SourceFile>, IndexError> {
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .follow_links(false)
            .hidden(true)
            .git_ignore(true)
            .require_git(false)
            .add_custom_ignore_filename(&self.options.folder_ignore_name);
        if let Some(global) = self.options.global_ignore.as_ref().filter(|path| path.is_file())
            && let Some(error) = builder.add_ignore(global)
        {
            return Err(IndexError::Pattern(format!("{}: {error}", global.display())));
        }
        if !self.options.excludes.is_empty() {
            let mut overrides = OverrideBuilder::new(&self.root);
            for pattern in &self.options.excludes {
                overrides
                    .add(&format!("!{pattern}"))
                    .map_err(|error| IndexError::Pattern(format!("{pattern}: {error}")))?;
            }
            builder.overrides(
                overrides
                    .build()
                    .map_err(|error| IndexError::Pattern(error.to_string()))?,
            );
        }
        let mut files: Vec<SourceFile> = builder
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .filter(|entry| {
                entry.path().extension().is_some_and(|extension| {
                    self.options
                        .extensions
                        .iter()
                        .any(|wanted| extension == wanted.as_str())
                })
            })
            .filter(|entry| {
                entry
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() <= self.options.maximum_file_bytes)
            })
            .filter_map(|entry| {
                Self::relative_to(&self.root, entry.path()).map(|relative| SourceFile {
                    absolute: entry.path().to_path_buf(),
                    relative,
                })
            })
            .collect();
        files.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(files)
    }

    fn relative_to(root: &Path, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(root).ok()?;
        Some(
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }
}
