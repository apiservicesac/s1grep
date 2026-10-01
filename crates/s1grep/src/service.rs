use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use s1_index::WalkOptions;
use serde::{Deserialize, Serialize};

use crate::indexer::Indexer;
use crate::models::{ModelDirectory, Retriever};
use crate::progress::IndexEvent;
use crate::project::{IndexLock, Project};
use crate::searcher::Searcher;
use crate::settings::{ConfigDirectory, IndexSettings, SearchSettings};

/// Which files a search reads: the ignore files decide, plus one-off patterns from the caller.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileFilters {
    pub excludes: Vec<String>,
}

impl FileFilters {
    pub fn walk_options(&self) -> anyhow::Result<WalkOptions> {
        Ok(WalkOptions {
            global_ignore: Some(ConfigDirectory::ignore_file()?),
            folder_ignore_name: IndexSettings::FOLDER_IGNORE_FILE.to_string(),
            excludes: self.excludes.clone(),
            maximum_file_bytes: IndexSettings::MAXIMUM_FILE_BYTES,
            extensions: IndexSettings::EXTENSIONS.iter().map(ToString::to_string).collect(),
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
        let target = std::fs::canonicalize(target).with_context(|| format!("{} does not exist", target.display()))?;
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
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    /// The project whose index answered; result paths are relative to it.
    pub root: PathBuf,
    /// The searched folder inside the project, if not the whole project.
    pub scope: Option<String>,
    /// Functions in the searched folder, and how many of them have a vector.
    pub functions: usize,
    pub indexed: usize,
    pub judged: usize,
    pub search_seconds: f64,
    pub results: Vec<SearchResult>,
}

impl SearchResponse {
    pub fn is_complete(&self) -> bool {
        self.indexed >= self.functions
    }
}

/// The models, loaded once, and the work of a search: bring the project's index up to date, retrieve, judge, fuse.
pub struct SearchService {
    searcher: Searcher,
    retriever: Retriever,
}

impl SearchService {
    pub fn load(models: &ModelDirectory, with_judge: bool, threads: Option<usize>) -> anyhow::Result<Self> {
        let retriever = Retriever::Granite;
        Ok(Self {
            searcher: Searcher::load(models, retriever, with_judge, threads)?,
            retriever,
        })
    }

    pub fn search(
        &mut self,
        request: &SearchRequest,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<SearchResponse> {
        let project = Project::locate(&request.target)?;
        let scope = project.scope.as_deref();
        let mut store = project.open_store()?;
        match IndexLock::acquire(&project.root)? {
            Ok(_lock) => {
                let mut indexer = Indexer {
                    store: &mut store,
                    retriever: self.retriever,
                };
                indexer.scan(&project.root, &request.filters.walk_options()?, progress)?;
                indexer.embed(&mut self.searcher.embedder, scope, progress)?;
            }
            Err(holder) => progress(IndexEvent::Busy { pid: holder }),
        }
        let coverage = store.coverage(self.retriever.key(), scope)?;
        let units = store.units_with_vectors(self.retriever.key(), scope)?;
        let judged = if self.searcher.has_judge() {
            request.judge_top
        } else {
            0
        };
        let started = Instant::now();
        let hits = self.searcher.search(&request.query, &units, judged)?;
        let search_seconds = started.elapsed().as_secs_f64();
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
                source: hit.unit.source,
            })
            .collect();
        Ok(SearchResponse {
            query: request.query.clone(),
            root: project.root.clone(),
            scope: project.scope.clone(),
            functions: coverage.units,
            indexed: coverage.embedded,
            judged: judged.min(units.len()),
            search_seconds,
            results,
        })
    }

    /// Brings a whole project up to date, for `s1grep index`.
    pub fn index(
        &mut self,
        target: &Path,
        filters: &FileFilters,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<Project> {
        let project = Project::locate(target)?;
        let mut store = project.open_store()?;
        let Ok(_lock) = IndexLock::acquire(&project.root)? else {
            anyhow::bail!(
                "another s1grep is indexing {} right now; `s1grep status` shows its progress",
                project.root.display()
            );
        };
        let mut indexer = Indexer {
            store: &mut store,
            retriever: self.retriever,
        };
        indexer.scan(&project.root, &filters.walk_options()?, progress)?;
        indexer.embed(&mut self.searcher.embedder, project.scope.as_deref(), progress)?;
        Ok(project)
    }

    pub fn retriever(&self) -> Retriever {
        self.retriever
    }
}
