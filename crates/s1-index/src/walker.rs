use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::overrides::OverrideBuilder;
use ignore::{DirEntry, WalkBuilder, WalkState};

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
        // Folders are walked in parallel; only accepted files touch the shared list.
        let found: Mutex<Vec<SourceFile>> = Mutex::new(Vec::new());
        builder.build_parallel().run(|| {
            let found = &found;
            Box::new(move |entry| {
                if let Ok(entry) = entry
                    && let Some(file) = self.accept(&entry)
                {
                    found.lock().expect("walker threads do not panic").push(file);
                }
                WalkState::Continue
            })
        });
        let mut files = found.into_inner().expect("walker threads do not panic");
        files.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(files)
    }

    /// The source file behind `entry`, if it is a file with a wanted extension and not too large to be code.
    fn accept(&self, entry: &DirEntry) -> Option<SourceFile> {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            return None;
        }
        let extension = entry.path().extension()?;
        if !self
            .options
            .extensions
            .iter()
            .any(|wanted| extension == wanted.as_str())
        {
            return None;
        }
        if !entry
            .metadata()
            .is_ok_and(|metadata| metadata.len() <= self.options.maximum_file_bytes)
        {
            return None;
        }
        Some(SourceFile {
            absolute: entry.path().to_path_buf(),
            relative: Self::relative_to(&self.root, entry.path())?,
        })
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

/// One-off exclude patterns applied to results, not to the index: excluding a folder for one search must not remove
/// it from the index other searches share.
pub struct PathExcludes {
    matcher: Option<ignore::overrides::Override>,
}

impl PathExcludes {
    pub fn new(root: &Path, patterns: &[String]) -> Result<Self, IndexError> {
        if patterns.is_empty() {
            return Ok(Self { matcher: None });
        }
        let mut builder = OverrideBuilder::new(root);
        for pattern in patterns {
            builder
                .add(&format!("!{pattern}"))
                .map_err(|error| IndexError::Pattern(format!("{pattern}: {error}")))?;
        }
        let matcher = builder
            .build()
            .map_err(|error| IndexError::Pattern(error.to_string()))?;
        Ok(Self { matcher: Some(matcher) })
    }

    /// Whether a unit at `relative` (a path under the root with `/` separators) is excluded, by its own path or by one
    /// of its folders.
    pub fn excludes(&self, relative: &str) -> bool {
        let Some(matcher) = &self.matcher else {
            return false;
        };
        let path = Path::new(relative);
        path.ancestors()
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .any(|ancestor| matcher.matched(ancestor, ancestor != path).is_ignore())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::PathExcludes;

    #[test]
    fn excludes_a_folder_and_what_is_inside_it() {
        let excludes =
            PathExcludes::new(Path::new("/project"), &["legacy/".to_string(), "*_old.py".to_string()]).unwrap();
        assert!(excludes.excludes("legacy/billing/invoice.py"));
        assert!(excludes.excludes("app/report_old.py"));
        assert!(!excludes.excludes("app/legacy_report.py"));
        assert!(!excludes.excludes("app/invoice.py"));
    }
}
