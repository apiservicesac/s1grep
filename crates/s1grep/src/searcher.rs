use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{
    CodeUnit, FusionWeights, IndexStore, PythonExtractor, RankFusion, SourceWalker, StoredUnit, VectorRanking,
    content_hash,
};
use serde_json::{Value, json};

use crate::models::{ModelDirectory, Retriever};

/// What an index refresh did.
#[derive(Debug, Default)]
pub struct RefreshReport {
    pub files: usize,
    pub changed_files: usize,
    pub removed_files: usize,
    pub units: usize,
    pub embedded_units: usize,
    pub seconds: f64,
}

/// Keeps a repository's index current: re-extracts changed files and embeds units that have no vector yet.
pub struct Indexer<'a> {
    pub store: &'a mut IndexStore,
    pub embedder: &'a mut Embedder,
    pub retriever: Retriever,
}

impl Indexer<'_> {
    const EMBED_BATCH: usize = 64;

    pub fn refresh(&mut self, root: &Path, include_tests: bool) -> anyhow::Result<RefreshReport> {
        let started = Instant::now();
        let mut report = RefreshReport::default();
        let mut extractor = PythonExtractor::new()?;
        let files = SourceWalker::new(root, include_tests).files();
        report.files = files.len();
        let seen: std::collections::HashSet<&str> = files.iter().map(|file| file.relative.as_str()).collect();
        for path in self.store.indexed_paths()? {
            if !seen.contains(path.as_str()) {
                self.store.remove_file(&path)?;
                report.removed_files += 1;
            }
        }
        for file in &files {
            let bytes =
                std::fs::read(&file.absolute).with_context(|| format!("reading {}", file.absolute.display()))?;
            let hash = content_hash(&bytes);
            if self.store.file_hash(&file.relative)?.as_deref() == Some(hash.as_str()) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let units = extractor.extract(&file.relative, &text);
            self.store.replace_file(&file.relative, &hash, &units)?;
            report.changed_files += 1;
        }
        let pending = self.store.units_without_vector(self.retriever.key())?;
        for chunk in pending.chunks(Self::EMBED_BATCH) {
            let texts: Vec<String> = chunk.iter().map(|stored| stored.unit.document_text()).collect();
            let vectors = self.embedder.embed_documents(&texts)?;
            let rows: Vec<(i64, Vec<f32>)> = chunk.iter().map(|stored| stored.id).zip(vectors).collect();
            self.store.store_vectors(self.retriever.key(), &rows)?;
            report.embedded_units += chunk.len();
        }
        report.units = self.store.unit_count()?;
        report.seconds = started.elapsed().as_secs_f64();
        Ok(report)
    }
}

/// One search result.
#[derive(Debug, Clone)]
pub struct Hit {
    pub unit: CodeUnit,
    pub retriever_rank: usize,
    pub similarity: f32,
    /// Probability that the code answers the search, when the judge read it.
    pub judge: Option<f64>,
}

/// The two-stage search: the embedder brings candidates, s1-code judges the first few, and both orders are fused.
pub struct Searcher {
    pub embedder: Embedder,
    judge: Option<LayaEngine>,
    retriever: Retriever,
}

impl Searcher {
    pub const TEMPLATE: &'static str = "This code answers the search: ";
    pub const CANDIDATES: usize = 25;

    pub fn load(
        models: &ModelDirectory,
        retriever: Retriever,
        with_judge: bool,
        threads: Option<usize>,
    ) -> anyhow::Result<Self> {
        let mut options = EngineOptions::default();
        if let Some(threads) = threads {
            options.threads = threads;
        }
        let embedder_bundle = EmbedderBundle::open(models.bundle(retriever.bundle_name())?)?;
        let embedder = Embedder::load(&embedder_bundle, options.threads, Accelerator::Cpu)?;
        let judge = if with_judge {
            let bundle = ModelBundle::open(models.bundle(ModelDirectory::JUDGE_BUNDLE)?)?;
            Some(LayaEngine::load(&bundle, &options)?)
        } else {
            None
        };
        Ok(Self {
            embedder,
            judge,
            retriever,
        })
    }

    pub fn has_judge(&self) -> bool {
        self.judge.is_some()
    }

    /// Candidates for `query` among `units`, best first, with the judge's reading of the first `judged`.
    pub fn search(&mut self, query: &str, units: &[(StoredUnit, Vec<f32>)], judged: usize) -> anyhow::Result<Vec<Hit>> {
        let query_vector = self.embedder.embed_query(query)?;
        let vectors: Vec<&[f32]> = units.iter().map(|(_, vector)| vector.as_slice()).collect();
        let nearest = VectorRanking::top(&query_vector, &vectors, Self::CANDIDATES);
        let mut hits: Vec<Hit> = nearest
            .iter()
            .enumerate()
            .map(|(rank, &(index, similarity))| Hit {
                unit: units[index].0.unit.clone(),
                retriever_rank: rank + 1,
                similarity,
                judge: None,
            })
            .collect();
        let Some(judge) = self.judge.as_mut() else {
            return Ok(hits);
        };
        let judged = judged.min(hits.len());
        if judged == 0 {
            return Ok(hits);
        }
        let questions = QuestionSet::from_json(&json!({
            "answers": {"type": "noul", "instructions": format!("{}{query}", Self::TEMPLATE)}
        }))?;
        // One candidate per run: a padded batch pays for its longest candidate on every row, which measured
        // 25 % slower on CPU with identical answers.
        let mut scores = Vec::with_capacity(judged);
        for hit in &hits[..judged] {
            let decisions = judge.decide_batch(&[Value::String(hit.unit.judge_state())], &questions)?;
            scores.push(decisions[0].answers[0].1.noul());
        }
        for (hit, score) in hits.iter_mut().zip(&scores) {
            hit.judge = Some(*score);
        }
        let order = RankFusion::order(hits.len(), &scores, FusionWeights::tuned(self.retriever.key(), judged));
        Ok(order.into_iter().map(|index| hits[index].clone()).collect())
    }
}
