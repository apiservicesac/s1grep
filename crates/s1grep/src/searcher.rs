use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{CodeUnit, FusionWeights, RankFusion, StoredUnit, VectorRanking};
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

    /// Candidates for `query` among `units`, best first, with the judge's reading of the first `judged`.
    pub fn search(&mut self, query: &str, units: &[(StoredUnit, Vec<f32>)], judged: usize) -> anyhow::Result<Vec<Hit>> {
        let query_vector = self.embedder.embed_query(query)?;
        let vectors: Vec<&[f32]> = units.iter().map(|(_, vector)| vector.as_slice()).collect();
        let nearest = VectorRanking::top(&query_vector, &vectors, SearchSettings::CANDIDATES);
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
            "answers": {"type": "noul", "instructions": format!("{}{query}", SearchSettings::QUESTION_TEMPLATE)}
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
