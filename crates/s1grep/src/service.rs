use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use s1_index::IndexStore;
use serde::{Deserialize, Serialize};

use crate::models::{CacheDirectory, ModelDirectory, Retriever};
use crate::searcher::{Indexer, Searcher};

/// One search, as the command line, the background server and the MCP server all pass it around.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Repository root; made absolute before the request leaves the caller.
    pub root: PathBuf,
    pub top: usize,
    /// Candidates the judge reads; 0 ranks by embeddings only.
    pub judge_top: usize,
    pub include_tests: bool,
}

impl SearchRequest {
    pub const DEFAULT_TOP: usize = 5;
    pub const MAXIMUM_TOP: usize = 25;

    pub fn new(query: &str, root: &Path, top: usize, judge_top: usize, include_tests: bool) -> anyhow::Result<Self> {
        let root = std::fs::canonicalize(root).with_context(|| format!("{} does not exist", root.display()))?;
        if !root.is_dir() {
            anyhow::bail!("{} is not a folder", root.display());
        }
        Ok(Self {
            query: query.trim().to_string(),
            root,
            top: top.clamp(1, Self::MAXIMUM_TOP),
            judge_top,
            include_tests,
        })
    }
}

/// One ranked function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub rank: usize,
    /// Path relative to the repository root.
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
    pub root: PathBuf,
    pub functions: usize,
    pub reindexed_files: usize,
    pub judged: usize,
    pub search_seconds: f64,
    pub results: Vec<SearchResult>,
}

/// The models, loaded once, and the work of a search: refresh the repository's index, retrieve, judge, fuse.
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

    pub fn search(&mut self, request: &SearchRequest) -> anyhow::Result<SearchResponse> {
        let mut store = IndexStore::open(&CacheDirectory::index_for(&request.root)?)?;
        let refresh = Indexer {
            store: &mut store,
            embedder: &mut self.searcher.embedder,
            retriever: self.retriever,
        }
        .refresh(&request.root, request.include_tests)?;
        let units = store.units_with_vectors(self.retriever.key())?;
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
            root: request.root.clone(),
            functions: units.len(),
            reindexed_files: refresh.changed_files,
            judged: judged.min(units.len()),
            search_seconds,
            results,
        })
    }
}
