use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{CodeUnit, FusionWeights, RankFusion, SearchableUnit, VectorRanking};
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
        })
    }

    pub fn has_judge(&self) -> bool {
        self.judge.is_some()
    }

    /// Candidates for `query` among `units`, best first, with the judge's reading of the first `judged`, and the
    /// judge's order fused with the retriever's (weights tuned on the dev exam).
    pub fn search(&mut self, query: &str, units: &[SearchableUnit], judged: usize) -> anyhow::Result<Vec<Hit>> {
        let mut hits = self.by_meaning(query, units)?;
        let judged = judged.min(hits.len());
        if self.judge.is_none() || judged == 0 {
            return Ok(hits);
        }
        let scores = self.judge(query, hits[..judged].iter().map(|hit| &hit.unit))?;
        for (hit, score) in hits.iter_mut().zip(&scores) {
            hit.judge = Some(*score);
        }
        let order = RankFusion::order(hits.len(), &scores, FusionWeights::tuned(self.retriever.key(), judged));
        Ok(order.into_iter().map(|index| hits[index].clone()).collect())
    }

    fn by_meaning(&mut self, query: &str, units: &[SearchableUnit]) -> anyhow::Result<Vec<Hit>> {
        if units.is_empty() {
            return Ok(Vec::new());
        }
        let query_vector = self.embedder.embed_query(query)?;
        let vectors: Vec<&[f32]> = units.iter().map(|searchable| searchable.vector.as_slice()).collect();
        Ok(VectorRanking::top(&query_vector, &vectors, SearchSettings::CANDIDATES)
            .into_iter()
            .enumerate()
            .map(|(rank, (index, similarity))| Hit {
                unit: units[index].stored.unit.clone(),
                retriever_rank: rank + 1,
                similarity,
                judge: None,
                found_by: if units[index].whole {
                    FoundBy::Code
                } else {
                    FoundBy::Outline
                },
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
            scores.push(decisions[0].answers[0].1.noul());
        }
        Ok(scores)
    }
}
