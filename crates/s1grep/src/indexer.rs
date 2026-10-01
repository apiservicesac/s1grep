use std::path::Path;
use std::time::{Duration, Instant, UNIX_EPOCH};

use s1_engine::Embedder;
use s1_index::{
    CodeUnit, ContentFingerprint, ExtractorRegistry, FileState, IndexLimits, IndexStore, SourceWalker, WalkOptions,
};

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
    /// The embedding space key of this pass's vectors.
    pub fn key(self, retriever: Retriever) -> String {
        match self {
            Self::Outline => retriever.outline_partner().space(self.text_format()).key(),
            Self::Whole => retriever.space(self.text_format()).key(),
        }
    }

    pub fn text_format(self) -> &'static str {
        match self {
            Self::Outline => IndexLimits::OUTLINE_TEXT_FORMAT,
            Self::Whole => IndexLimits::WHOLE_TEXT_FORMAT,
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
    /// Files that disappeared or could not be read between the walk and the read.
    pub skipped: usize,
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
        let mut extractors = ExtractorRegistry::new()?;
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
            // A file that vanished or cannot be read since the walk is skipped, not a reason to fail the search.
            let Ok(metadata) = std::fs::metadata(&file.absolute) else {
                report.skipped += 1;
                continue;
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_nanos() as i64);
            let Some(extractor) = extractors.version_for(&file.relative) else {
                continue;
            };
            let known = known_files.remove(&file.relative);
            if known.as_ref().is_some_and(|state| {
                state.size == metadata.len() && state.modified == modified && state.extractor == extractor
            }) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&file.absolute) else {
                report.skipped += 1;
                continue;
            };
            let state = FileState {
                hash: ContentFingerprint::of(&bytes),
                size: metadata.len(),
                modified,
                extractor,
            };
            if known.is_some_and(|previous| previous.hash == state.hash && previous.extractor == state.extractor) {
                self.store.touch_file(&file.relative, &state)?;
                continue;
            }
            let units = extractors.extract(&file.relative, &String::from_utf8_lossy(&bytes));
            self.store.replace_file(&file.relative, &state, &units)?;
            report.changed += 1;
        }
        self.store.commit_batch()?;
        progress(IndexEvent::Scanned {
            files: report.files,
            changed: report.changed,
            skipped: report.skipped,
            functions: self.store.unit_count()?,
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
        self.embed_for(embedder, pass, scope, None, progress)
    }

    /// Like `embed`, but stops after the batch that crosses `budget`, leaving the rest for later.
    pub fn embed_for(
        &mut self,
        embedder: &mut Embedder,
        pass: Pass,
        scope: Option<&str>,
        budget: Option<Duration>,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<usize> {
        let key = pass.key(self.retriever);
        let total = self.store.pending_count(&key, scope)?;
        let started = Instant::now();
        let mut done = 0;
        while done < total && budget.is_none_or(|budget| started.elapsed() < budget) {
            let batch = self.store.pending_units_within(&key, scope, pass.batch())?;
            if batch.is_empty() {
                break;
            }
            let texts: Vec<String> = batch.iter().map(|stored| pass.text(&stored.unit)).collect();
            let vectors = embedder.embed_documents(&texts)?;
            let rows: Vec<(String, Vec<f32>)> =
                batch.iter().map(|stored| stored.content.clone()).zip(vectors).collect();
            self.store.store_vectors(&key, &rows)?;
            done = (done + rows.len()).min(total);
            progress(pass.event(done, total, started.elapsed().as_secs_f64()));
        }
        Ok(done)
    }
}
