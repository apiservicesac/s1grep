use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{CodeUnit, FusionWeights, IndexStore, RankFusion, VectorIndex, VectorRow};
use serde_json::{Value, json};

use crate::models::{ModelDirectory, Retriever};
use crate::settings::SearchSettings;

/// One search result.
#[derive(Debug, Clone)]
pub struct Hit {
    pub unit: CodeUnit,
    pub retriever_rank: usize,
    pub similarity: f32,
    /// Probability that the code answers the search, when the judge read it.
    pub judge: Option<f64>,
    pub found_by: FoundBy,
}

/// Which vector found a result: the one of its whole source, or the outline one (path, name and signature) that a
/// function has while its project is still being indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    Code,
    Outline,
}

/// The two-stage search: the embedder brings candidates, s1-code judges the first few, and both orders are fused.
pub struct Searcher {
    pub embedder: Embedder,
    judge: Option<LayaEngine>,
    retriever: Retriever,
    /// The judge stops reading candidates once one scores at least this; `None` reads them all.
    early_stop: Option<f64>,
}

impl Searcher {
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
            let bundle = ModelBundle::open(models.bundle(crate::settings::ModelSettings::JUDGE_BUNDLE)?)?;
            Some(LayaEngine::load(&bundle, &options)?)
        } else {
            None
        };
        Ok(Self {
            embedder,
            judge,
            retriever,
            early_stop: Some(SearchSettings::JUDGE_EARLY_STOP),
        })
    }

    pub fn set_early_stop(&mut self, threshold: Option<f64>) {
        self.early_stop = threshold;
    }

    pub fn has_judge(&self) -> bool {
        self.judge.is_some()
    }

    /// The functions that answer `query` best: the nearest rows of `index` that `keep` accepts, read from `store`,
    /// with the judge's reading of the first `judged`, and the judge's order fused with the retriever's (weights
    /// tuned on the development exam).
    pub fn search(
        &mut self,
        query: &str,
        index: &VectorIndex,
        keep: impl Fn(&VectorRow) -> bool,
        store: &IndexStore,
        judged: usize,
    ) -> anyhow::Result<Vec<Hit>> {
        let mut hits = self.by_meaning(query, index, keep, store)?;
        let judged = judged.min(hits.len());
        if self.judge.is_none() || judged == 0 {
            return Ok(hits);
        }
        let scores = self.judge(query, hits[..judged].iter().map(|hit| &hit.unit))?;
        for (hit, score) in hits.iter_mut().zip(&scores) {
            hit.judge = Some(*score);
        }
        let order = RankFusion::order(hits.len(), &scores, FusionWeights::tuned(self.retriever.key(), judged));
        Ok(order.into_iter().map(|position| hits[position].clone()).collect())
    }

    fn by_meaning(
        &mut self,
        query: &str,
        index: &VectorIndex,
        keep: impl Fn(&VectorRow) -> bool,
        store: &IndexStore,
    ) -> anyhow::Result<Vec<Hit>> {
        if index.is_empty() {
            return Ok(Vec::new());
        }
        let query_vector = self.embedder.embed_query(query)?;
        let nearest = index.nearest(&query_vector, SearchSettings::CANDIDATES, keep);
        let ids: Vec<i64> = nearest
            .iter()
            .map(|(position, _)| index.row(*position).unit_id)
            .collect();
        let units = store.units_by_ids(&ids)?;
        Ok(nearest
            .iter()
            .filter_map(|(position, similarity)| {
                let row = index.row(*position);
                let stored = units.iter().find(|stored| stored.id == row.unit_id)?;
                Some((row, stored, *similarity))
            })
            .enumerate()
            .map(|(rank, (row, stored, similarity))| Hit {
                unit: stored.unit.clone(),
                retriever_rank: rank + 1,
                similarity,
                judge: None,
                found_by: if row.whole { FoundBy::Code } else { FoundBy::Outline },
            })
            .collect())
    }

    /// The judge's probability that each unit answers `query`. One candidate per run: a padded batch pays for its
    /// longest candidate on every row, which measured 25 % slower on CPU with identical answers.
    fn judge<'a>(&mut self, query: &str, units: impl Iterator<Item = &'a CodeUnit>) -> anyhow::Result<Vec<f64>> {
        let Some(judge) = self.judge.as_mut() else {
            return Ok(Vec::new());
        };
        let questions = QuestionSet::from_json(&json!({
            "answers": {"type": "noul", "instructions": format!("{}{query}", SearchSettings::QUESTION_TEMPLATE)}
        }))?;
        let mut scores = Vec::new();
        for unit in units {
            let decisions = judge.decide_batch(&[Value::String(unit.judge_state())], &questions)?;
            let score = decisions[0].answers[0].1.noul();
            scores.push(score);
            if self.early_stop.is_some_and(|threshold| score >= threshold) {
                break;
            }
        }
        Ok(scores)
    }
}
