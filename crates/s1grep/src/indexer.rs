use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use anyhow::Context;
use s1_engine::Embedder;
use s1_index::{CodeUnit, ContentFingerprint, FileState, IndexStore, PythonExtractor, SourceWalker, WalkOptions};

use crate::models::Retriever;
use crate::progress::IndexEvent;
use crate::settings::IndexSettings;

/// The two vectors a function gets: first one of its outline (path, name, signature), cheap enough to cover a large
/// project in about a minute, then one of its whole source, computed in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Outline,
    Whole,
}

impl Pass {
    pub fn key(self, retriever: Retriever) -> &'static str {
        match self {
            Self::Outline => retriever.outline_key(),
            Self::Whole => retriever.key(),
        }
    }

    pub fn text(self, unit: &CodeUnit) -> String {
        match self {
            Self::Outline => unit.outline_text(),
            Self::Whole => unit.document_text(),
        }
    }

    pub fn batch(self) -> usize {
        match self {
            Self::Outline => IndexSettings::OUTLINE_BATCH,
            Self::Whole => IndexSettings::EMBED_BATCH,
        }
    }

    fn event(self, done: usize, total: usize, seconds: f64) -> IndexEvent {
        match self {
            Self::Outline => IndexEvent::Outlining { done, total, seconds },
            Self::Whole => IndexEvent::Embedding { done, total, seconds },
        }
    }
}

/// What a scan changed.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScanReport {
    pub files: usize,
    pub changed: usize,
    pub removed: usize,
}

/// Keeps a project's index current in two steps: `scan` mirrors the files into functions (seconds, even for large
/// projects), and `embed` computes the missing vectors (the slow part), in short batches that report progress.
pub struct Indexer<'a> {
    pub store: &'a mut IndexStore,
    pub retriever: Retriever,
}

impl Indexer<'_> {
    pub fn scan(
        &mut self,
        root: &Path,
        options: &WalkOptions,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<ScanReport> {
        let started = Instant::now();
        let files = SourceWalker::new(root, options.clone()).files()?;
        let mut report = ScanReport {
            files: files.len(),
            ..ScanReport::default()
        };
        let seen: std::collections::HashSet<&str> = files.iter().map(|file| file.relative.as_str()).collect();
        for path in self.store.indexed_paths()? {
            if !seen.contains(path.as_str()) {
                self.store.remove_file(&path)?;
                report.removed += 1;
            }
        }
        let mut extractor = PythonExtractor::new()?;
        let mut known_files = self.store.file_states()?;
        self.store.begin_batch()?;
        for (index, file) in files.iter().enumerate() {
            if index > 0 && index % IndexSettings::SCAN_COMMIT_EVERY == 0 {
                self.store.commit_batch()?;
                self.store.begin_batch()?;
            }
            if index % IndexSettings::SCAN_REPORT_EVERY == 0 {
                progress(IndexEvent::Scanning {
                    done: index,
                    files: files.len(),
                });
            }
            let metadata =
                std::fs::metadata(&file.absolute).with_context(|| format!("reading {}", file.absolute.display()))?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_nanos() as i64);
            let known = known_files.remove(&file.relative);
            if known
                .as_ref()
                .is_some_and(|state| state.size == metadata.len() && state.modified == modified)
            {
                continue;
            }
            let bytes =
                std::fs::read(&file.absolute).with_context(|| format!("reading {}", file.absolute.display()))?;
            let state = FileState {
                hash: ContentFingerprint::of(&bytes),
                size: metadata.len(),
                modified,
            };
            if known.is_some_and(|previous| previous.hash == state.hash) {
                self.store.touch_file(&file.relative, &state)?;
                continue;
            }
            let units = extractor.extract(&file.relative, &String::from_utf8_lossy(&bytes));
            self.store.replace_file(&file.relative, &state, &units)?;
            report.changed += 1;
        }
        self.store.commit_batch()?;
        progress(IndexEvent::Scanned {
            files: report.files,
            changed: report.changed,
            functions: self.store.coverage(self.retriever.key(), None)?.units,
            seconds: started.elapsed().as_secs_f64(),
        });
        Ok(report)
    }

    /// Computes the missing vectors of `pass` under `scope` (the whole project when `None`). Returns how many.
    pub fn embed(
        &mut self,
        embedder: &mut Embedder,
        pass: Pass,
        scope: Option<&str>,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<usize> {
        let key = pass.key(self.retriever);
        let total = self.store.pending_count(key, scope)?;
        let started = Instant::now();
        let mut done = 0;
        while done < total {
            let batch = self.store.pending_units(key, scope, pass.batch())?;
            let batch: Vec<_> = match scope {
                Some(scope) => batch
                    .into_iter()
                    .filter(|stored| Self::inside(&stored.unit.path, scope))
                    .collect(),
                None => batch,
            };
            if batch.is_empty() {
                break;
            }
            let texts: Vec<String> = batch.iter().map(|stored| pass.text(&stored.unit)).collect();
            let vectors = embedder.embed_documents(&texts)?;
            let rows: Vec<(String, Vec<f32>)> =
                batch.iter().map(|stored| stored.content.clone()).zip(vectors).collect();
            self.store.store_vectors(key, &rows)?;
            done = (done + rows.len()).min(total);
            progress(pass.event(done, total, started.elapsed().as_secs_f64()));
        }
        Ok(done)
    }

    fn inside(path: &str, scope: &str) -> bool {
        path == scope || path.strip_prefix(scope).is_some_and(|rest| rest.starts_with('/'))
    }
}
