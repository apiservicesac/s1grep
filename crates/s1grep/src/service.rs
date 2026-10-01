use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use s1_index::{LanguageSettings, PathExcludes, VectorRow, WalkOptions};
use serde::{Deserialize, Serialize};

use crate::cache::{ProjectFolder, ProjectInfo};
use crate::indexer::{Indexer, Pass};
use crate::models::{ModelDirectory, Retriever};
use crate::progress::IndexEvent;
use crate::project::{IndexLock, Project};
use crate::searcher::{FoundBy, Searcher};
use crate::session::ProjectSessions;
use crate::settings::{ConfigDirectory, IndexSettings, SearchSettings};

/// Which files a search reads: the ignore files decide what is indexed, and one-off patterns from the caller hide
/// results of this search only, without touching the index other searches share.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileFilters {
    pub excludes: Vec<String>,
}

impl FileFilters {
    /// What the index holds: the ignore files only.
    pub fn walk_options(&self) -> anyhow::Result<WalkOptions> {
        Ok(WalkOptions {
            global_ignore: Some(ConfigDirectory::ignore_file()?),
            folder_ignore_name: IndexSettings::FOLDER_IGNORE_FILE.to_string(),
            excludes: Vec::new(),
            maximum_file_bytes: IndexSettings::MAXIMUM_FILE_BYTES,
            extensions: LanguageSettings::extensions(),
        })
    }
}

/// One search, as the command line, the background server and the MCP server all pass it around.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// The folder to search, absolute; it may sit inside a larger project that is already indexed.
    pub target: PathBuf,
    pub top: usize,
    /// Candidates the judge reads; 0 ranks by embeddings only.
    pub judge_top: usize,
    pub filters: FileFilters,
}

impl SearchRequest {
    pub fn new(query: &str, target: &Path, top: usize, judge_top: usize, filters: FileFilters) -> anyhow::Result<Self> {
        let target = dunce::canonicalize(target).with_context(|| format!("{} does not exist", target.display()))?;
        if !target.is_dir() {
            anyhow::bail!("{} is not a folder", target.display());
        }
        Ok(Self {
            query: query.trim().to_string(),
            target,
            top: top.clamp(1, SearchSettings::MAXIMUM_TOP),
            judge_top,
            filters,
        })
    }
}

/// One ranked function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub rank: usize,
    /// Path relative to the project root.
    pub path: String,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Probability that the code answers the search, when the judge read it.
    pub judge: Option<f64>,
    pub similarity: f32,
    pub found_by: FoundBy,
    pub source: String,
}

/// How far the background process has indexed a project that is still being indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingProgress {
    pub done: usize,
    pub total: usize,
    pub seconds_left: Option<f64>,
    /// Time until every function has at least an outline vector, i.e. the whole project can be searched.
    #[serde(default)]
    pub searchable_seconds_left: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    /// The project whose index answered; result paths are relative to it.
    pub root: PathBuf,
    /// The searched folder inside the project, if not the whole project.
    pub scope: Option<String>,
    /// Functions in the searched folder, how many have a vector of their whole source, and how many could be searched
    /// at all (the rest of them through their outline vector).
    pub functions: usize,
    pub indexed: usize,
    pub searchable: usize,
    /// Present while the background process is still indexing this project.
    pub indexing: Option<IndexingProgress>,
    pub judged: usize,
    pub search_seconds: f64,
    pub results: Vec<SearchResult>,
}

impl SearchResponse {
    pub fn is_complete(&self) -> bool {
        self.indexed >= self.functions
    }
}

/// Functions of a project still missing a vector, for each pass.
struct PendingWork {
    whole: usize,
    outline: usize,
}

/// A project the background process keeps indexing between searches, holding its lock until it is done.
struct IndexingJob {
    project: Project,
    /// Held until the job is done, so no other process indexes the project meanwhile.
    #[expect(dead_code, reason = "held only to release the lock when the job is dropped")]
    index_lock: IndexLock,
    /// Outlines of the whole project first, then whole sources.
    pass: Pass,
    /// Functions fetched from the index and not embedded yet; refilled when empty, emptied when the priority changes.
    queue: Vec<s1_index::StoredUnit>,
    started: Instant,
    /// When the first batch was embedded; the pace is measured from here, not from when the job was scheduled.
    first_step: Option<Instant>,
    done: usize,
    total: usize,
    /// The same for the outline pass, which makes the whole project searchable.
    outline_first_step: Option<Instant>,
    outline_done: usize,
    outline_total: usize,
    /// Consecutive failed steps; the job is dropped after `IndexSettings::JOB_ATTEMPTS` of them.
    failures: u32,
}

impl IndexingJob {
    fn progress(&self) -> IndexingProgress {
        IndexingProgress {
            done: self.done,
            total: self.total,
            seconds_left: Self::time_left(self.first_step, self.done, self.total),
            searchable_seconds_left: if self.pass == Pass::Outline {
                Self::time_left(self.outline_first_step, self.outline_done, self.outline_total)
            } else {
                None
            },
        }
    }

    /// Time left at the pace measured since `first_step`, once enough is done for the pace to mean something.
    fn time_left(first_step: Option<Instant>, done: usize, total: usize) -> Option<f64> {
        let elapsed = first_step.map_or(0.0, |first| first.elapsed().as_secs_f64());
        let measured = done >= IndexSettings::ESTIMATE_AFTER && elapsed > 0.0;
        let rate = if measured { done as f64 / elapsed } else { 0.0 };
        (rate > 0.0).then(|| total.saturating_sub(done) as f64 / rate)
    }
}

/// The models, loaded once, and the work of a search: bring the project's index up to date, retrieve, judge, fuse.
/// A project with many functions still to index first gets outline vectors for the searched folder (a short wait,
/// once), is searched with them, and is handed to `index_step`, which the background process calls between searches.
pub struct SearchService {
    searcher: Searcher,
    retriever: Retriever,
    sessions: ProjectSessions,
    jobs: Vec<IndexingJob>,
}

impl SearchService {
    pub fn load(models: &ModelDirectory, with_judge: bool, threads: Option<usize>) -> anyhow::Result<Self> {
        let retriever = Retriever::Granite;
        Ok(Self {
            searcher: Searcher::load(models, retriever, with_judge, threads)?,
            retriever,
            sessions: ProjectSessions::default(),
            jobs: Vec::new(),
        })
    }

    pub fn search(
        &mut self,
        request: &SearchRequest,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<SearchResponse> {
        let project = Project::locate(&request.target)?;
        let scope = project.scope.as_deref();
        project.record_use()?;
        let session = self.sessions.get(&project)?;
        let has_job = self.jobs.iter().any(|job| job.project.root == project.root);
        let lock = if has_job {
            None
        } else {
            match IndexLock::acquire(&project.folder)? {
                Ok(lock) => Some(lock),
                Err(holder) => {
                    progress(IndexEvent::Busy { pid: holder });
                    None
                }
            }
        };
        let whole = Pass::Whole.key(self.retriever);
        let mut total_pending = 0;
        let mut outline_pending = 0;
        if has_job || lock.is_some() {
            let mut indexer = Indexer {
                store: &mut session.store,
                retriever: self.retriever,
            };
            let report = indexer.scan(&project.root, &request.filters.walk_options()?, progress)?;
            let pending = indexer.store.pending_count(&whole, scope)?;
            let embedded = if pending > 0 && pending <= IndexSettings::INDEX_BEFORE_ANSWERING {
                indexer.embed(self.searcher.embedder_for(Pass::Whole), Pass::Whole, scope, progress)?
            } else if pending > 0 && !has_job {
                // Only the first search waits, briefly; later ones answer at once while the background works.
                indexer.embed_for(
                    self.searcher.embedder_for(Pass::Outline),
                    Pass::Outline,
                    scope,
                    Some(IndexSettings::OUTLINE_BEFORE_ANSWERING),
                    progress,
                )?
            } else {
                0
            };
            // Whatever is still missing, here or elsewhere in the project, is left to the background process.
            total_pending = indexer.store.pending_count(&whole, None)?;
            outline_pending = indexer.store.pending_count(&Pass::Outline.key(self.retriever), None)?;
            if report.changed > 0 || report.removed > 0 || embedded > 0 {
                session.mark_changed();
            }
        }
        let coverage = session.store.coverage(&whole, scope)?;
        let excludes = PathExcludes::new(&project.root, &request.filters.excludes)?;
        let keep = |row: &VectorRow| {
            scope.is_none_or(|scope| row.path == scope || row.path.starts_with(&format!("{scope}/")))
                && !excludes.excludes(&row.path)
        };
        let judged = if self.searcher.has_judge() {
            request.judge_top
        } else {
            0
        };
        let (vectors, store) = session.vectors(self.retriever)?;
        let searchable = vectors.rows().filter(|row| keep(row)).count();
        let started = Instant::now();
        let hits = self.searcher.search(&request.query, vectors, keep, store, judged)?;
        let search_seconds = started.elapsed().as_secs_f64();
        let judged = hits.iter().filter(|hit| hit.judge.is_some()).count();
        if total_pending > 0 {
            self.schedule(
                &project,
                lock,
                PendingWork {
                    whole: total_pending,
                    outline: outline_pending,
                },
            );
        }
        let results = hits
            .into_iter()
            .take(request.top)
            .enumerate()
            .map(|(index, hit)| SearchResult {
                rank: index + 1,
                path: hit.unit.path,
                name: hit.unit.name,
                start_line: hit.unit.start_line,
                end_line: hit.unit.end_line,
                judge: hit.judge,
                similarity: hit.similarity,
                found_by: hit.found_by,
                source: hit.unit.source,
            })
            .collect();
        Ok(SearchResponse {
            query: request.query.clone(),
            root: project.root.clone(),
            scope: project.scope.clone(),
            functions: coverage.units,
            indexed: coverage.embedded,
            searchable,
            indexing: self
                .jobs
                .iter()
                .find(|job| job.project.root == project.root)
                .map(IndexingJob::progress),
            judged,
            search_seconds,
            results,
        })
    }

    /// Hands a project to the background indexing, the searched folder first.
    fn schedule(&mut self, project: &Project, lock: Option<IndexLock>, totals: PendingWork) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.project.root == project.root) {
            if job.project.scope != project.scope {
                job.project.scope = project.scope.clone();
                job.queue.clear();
            }
            return;
        }
        if let Some(lock) = lock {
            self.jobs.push(IndexingJob {
                project: project.clone(),
                index_lock: lock,
                pass: Pass::Outline,
                queue: Vec::new(),
                started: Instant::now(),
                first_step: None,
                done: 0,
                total: totals.whole,
                outline_first_step: None,
                outline_done: 0,
                outline_total: totals.outline,
                failures: 0,
            });
        }
    }

    /// Resumes background indexing of projects searched recently whose vectors are not complete, so that a restart
    /// (a crash, an update, `s1grep stop`) does not leave them half done until someone searches there again.
    pub fn resume_pending(&mut self) -> anyhow::Result<usize> {
        let since = ProjectInfo::now().saturating_sub(IndexSettings::RESUME_WITHIN.as_secs());
        let whole = Pass::Whole.key(self.retriever);
        let mut resumed = 0;
        for folder in ProjectFolder::all()? {
            let Some(info) = folder.info().filter(|info| info.last_used >= since) else {
                continue;
            };
            if !Path::new(&info.root).is_dir() {
                continue;
            }
            let project = Project::at_root(Path::new(&info.root))?;
            let Ok(lock) = IndexLock::acquire(&project.folder)? else {
                continue;
            };
            let store = &self.sessions.get(&project)?.store;
            let totals = PendingWork {
                whole: store.pending_count(&whole, None)?,
                outline: store.pending_count(&Pass::Outline.key(self.retriever), None)?,
            };
            if totals.whole > 0 {
                self.schedule(&project, Some(lock), totals);
                resumed += 1;
            }
        }
        Ok(resumed)
    }

    pub fn has_indexing(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Embeds one short batch of the oldest indexing job, so that searches never wait long for the background work.
    /// A job that keeps failing (an index deleted under it, a full disk) is dropped instead of retried forever.
    pub fn index_step(&mut self) -> anyhow::Result<()> {
        let result = self.step_oldest_job();
        if let Some(job) = self.jobs.first_mut() {
            match &result {
                Ok(()) => job.failures = 0,
                Err(_) => job.failures += 1,
            }
            if job.failures >= IndexSettings::JOB_ATTEMPTS {
                let dropped = self.jobs.remove(0);
                return result.with_context(|| {
                    format!(
                        "stopped indexing {} after {} failures; the next search there tries again",
                        dropped.project.root.display(),
                        IndexSettings::JOB_ATTEMPTS
                    )
                });
            }
        }
        result
    }

    fn step_oldest_job(&mut self) -> anyhow::Result<()> {
        let Some(job) = self.jobs.first_mut() else {
            return Ok(());
        };
        let session = self.sessions.get(&job.project)?;
        let store = &mut session.store;
        let key = job.pass.key(self.retriever);
        if job.queue.is_empty() {
            let batch = match job.pass {
                Pass::Outline => IndexSettings::BACKGROUND_OUTLINE_QUEUE,
                Pass::Whole => IndexSettings::BACKGROUND_QUEUE,
            };
            job.queue = store.pending_units_scope_first(&key, job.project.scope.as_deref(), batch)?;
            job.queue.reverse();
        }
        if job.queue.is_empty() {
            if job.pass == Pass::Outline {
                job.pass = Pass::Whole;
                return Ok(());
            }
            let finished = self.jobs.remove(0);
            eprintln!(
                "s1grep: indexed {} in {:.0} s",
                finished.project.root.display(),
                finished.started.elapsed().as_secs_f64()
            );
            return Ok(());
        }
        let take = match job.pass {
            Pass::Outline => IndexSettings::BACKGROUND_OUTLINE_BATCH,
            Pass::Whole => IndexSettings::BACKGROUND_BATCH,
        }
        .min(job.queue.len());
        let batch: Vec<_> = job.queue.split_off(job.queue.len() - take);
        let texts: Vec<String> = batch.iter().map(|stored| job.pass.text(&stored.unit)).collect();
        let vectors = self.searcher.embedder_for(job.pass).embed_documents(&texts)?;
        let rows: Vec<(String, Vec<f32>)> = batch.iter().map(|stored| stored.content.clone()).zip(vectors).collect();
        store.store_vectors(&key, &rows)?;
        session.mark_changed();
        match job.pass {
            Pass::Whole => {
                job.first_step.get_or_insert_with(Instant::now);
                job.done += rows.len();
            }
            Pass::Outline => {
                job.outline_first_step.get_or_insert_with(Instant::now);
                job.outline_done += rows.len();
            }
        }
        Ok(())
    }

    /// Brings a whole project up to date, for `s1grep index`.
    pub fn index(&mut self, target: &Path, progress: &mut dyn FnMut(IndexEvent)) -> anyhow::Result<Project> {
        let project = Project::locate(target)?;
        let mut store = project.open_store()?;
        let Ok(index_lock) = IndexLock::acquire(&project.folder)? else {
            anyhow::bail!(
                "another s1grep is indexing {} right now; `s1grep status` shows its progress",
                project.root.display()
            );
        };
        let mut indexer = Indexer {
            store: &mut store,
            retriever: self.retriever,
        };
        indexer.scan(&project.root, &FileFilters::default().walk_options()?, progress)?;
        indexer.embed(
            self.searcher.embedder_for(Pass::Whole),
            Pass::Whole,
            project.scope.as_deref(),
            progress,
        )?;
        drop(index_lock);
        Ok(project)
    }

    pub fn retriever(&self) -> Retriever {
        self.retriever
    }
}
