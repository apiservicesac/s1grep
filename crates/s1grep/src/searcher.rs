use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{CodeUnit, FusionWeights, IndexStore, LexicalIndex, RankFusion, VectorIndex, VectorRow};

use crate::indexer::Pass;
use serde_json::{Value, json};

use crate::models::{ModelDirectory, Retriever};
use crate::query::QueryShape;
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

/// What found a result: the vector of its whole source, its outline vector (while its project is still being
/// indexed), or only its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    Code,
    Outline,
    Words,
}

/// What a search ranks: the vectors of whole sources, and the outline vectors of the functions that have no
/// whole-source vector yet. The two come from different models, so they are ranked apart and their lists merged.
pub struct SearchIndexes {
    pub whole: VectorIndex,
    pub outline: VectorIndex,
}

impl SearchIndexes {
    pub fn rows(&self) -> impl Iterator<Item = &VectorRow> {
        self.whole.rows().iter().chain(self.outline.rows())
    }
}

/// One candidate before its source is read: how it was found, its similarity when a vector found it, and its
/// reciprocal-rank score summed over the lists that found it.
struct Candidate {
    unit_id: i64,
    found_by: FoundBy,
    similarity: f32,
    score: f64,
}

/// The two-stage search: the embedders bring candidates, s1-code judges the first few, and both orders are fused.
pub struct Searcher {
    /// Embeds whole sources and the query compared with them.
    embedder: Embedder,
    /// A smaller model for outlines, the quick first pass that makes a large project searchable in minutes.
    outline_embedder: Embedder,
    judge: Option<LayaEngine>,
    retriever: Retriever,
    /// The judge stops reading candidates once one scores at least this; `None` reads them all.
    early_stop: Option<f64>,
    /// Weight of the word index's list in the merge; 0 leaves it out.
    lexical_weight: f64,
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
        let outline_bundle = EmbedderBundle::open(models.bundle(retriever.outline_partner().bundle_name())?)?;
        let outline_embedder = Embedder::load(&outline_bundle, options.threads, Accelerator::Cpu)?;
        let judge = if with_judge {
            let bundle = ModelBundle::open(models.bundle(crate::settings::ModelSettings::JUDGE_BUNDLE)?)?;
            Some(LayaEngine::load(&bundle, &options)?)
        } else {
            None
        };
        Ok(Self {
            embedder,
            outline_embedder,
            judge,
            retriever,
            early_stop: Some(SearchSettings::JUDGE_EARLY_STOP),
            lexical_weight: SearchSettings::LEXICAL_WEIGHT,
        })
    }

    pub fn set_early_stop(&mut self, threshold: Option<f64>) {
        self.early_stop = threshold;
    }

    pub fn set_lexical_weight(&mut self, weight: f64) {
        self.lexical_weight = weight;
    }

    pub fn has_judge(&self) -> bool {
        self.judge.is_some()
    }

    /// The model that embeds the texts of `pass`.
    pub fn embedder_for(&mut self, pass: Pass) -> &mut Embedder {
        match pass {
            Pass::Whole => &mut self.embedder,
            Pass::Outline => &mut self.outline_embedder,
        }
    }

    /// The functions that answer `query` best: the nearest functions by meaning in `indexes` and the best by words
    /// in `lexical`, among those whose path `keep` accepts, read from `store`, with the judge's reading of the first
    /// `judged`, and the judge's order fused with the retrieval order (weights tuned on the development exam).
    pub fn search(
        &mut self,
        query: &str,
        indexes: &SearchIndexes,
        lexical: Option<&LexicalIndex>,
        keep: impl Fn(&str) -> bool,
        store: &IndexStore,
        judged: usize,
    ) -> anyhow::Result<Vec<Hit>> {
        let mut hits = self.candidates(query, indexes, lexical, keep, store)?;
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

    /// Up to `SearchSettings::CANDIDATES` functions merged from three lists by reciprocal rank: the nearest by
    /// whole-source vectors, by outline vectors (functions not fully indexed yet) and by words. A function found by
    /// several lists adds their scores. With a single list its order is kept as it is.
    fn candidates(
        &mut self,
        query: &str,
        indexes: &SearchIndexes,
        lexical: Option<&LexicalIndex>,
        keep: impl Fn(&str) -> bool,
        store: &IndexStore,
    ) -> anyhow::Result<Vec<Hit>> {
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut add = |unit_id: i64, found_by: FoundBy, similarity: f32, rank: usize, weight: f64| {
            let score = weight / (SearchSettings::MERGE_SMOOTHING + rank as f64 + 1.0);
            match candidates.iter_mut().find(|candidate| candidate.unit_id == unit_id) {
                Some(candidate) => candidate.score += score,
                None => candidates.push(Candidate {
                    unit_id,
                    found_by,
                    similarity,
                    score,
                }),
            }
        };
        for (index, embedder) in [
            (&indexes.whole, &mut self.embedder),
            (&indexes.outline, &mut self.outline_embedder),
        ] {
            if index.is_empty() {
                continue;
            }
            let query_vector = embedder.embed_query(query)?;
            let nearest = index.nearest(&query_vector, SearchSettings::CANDIDATES, |row| keep(&row.path));
            for (rank, (position, similarity)) in nearest.into_iter().enumerate() {
                let row = index.row(position);
                let found_by = if row.whole { FoundBy::Code } else { FoundBy::Outline };
                add(row.unit_id, found_by, similarity, rank, 1.0);
            }
        }
        // Searches written like code look for names: every word counts. Prose would only add noise (the code is
        // often in another language than the question), so it uses the word index only for exact phrases, such as
        // a pasted error message.
        if let Some(lexical) = lexical.filter(|_| self.lexical_weight > 0.0) {
            let hits = if QueryShape::looks_like_code(query) {
                lexical.search(query, SearchSettings::CANDIDATES, &keep)?
            } else {
                lexical.search_phrase(query, SearchSettings::CANDIDATES, &keep)?
            };
            for (rank, hit) in hits.into_iter().enumerate() {
                add(hit.unit_id, FoundBy::Words, 0.0, rank, self.lexical_weight);
            }
        }
        // Stable sort: on equal scores the earlier list (whole sources, then outlines, then words) stays first.
        candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
        candidates.truncate(SearchSettings::CANDIDATES);
        let ids: Vec<i64> = candidates.iter().map(|candidate| candidate.unit_id).collect();
        let units = store.units_by_ids(&ids)?;
        Ok(candidates
            .iter()
            .filter_map(|candidate| {
                let stored = units.iter().find(|stored| stored.id == candidate.unit_id)?;
                Some((candidate, stored))
            })
            .enumerate()
            .map(|(rank, (candidate, stored))| Hit {
                unit: stored.unit.clone(),
                retriever_rank: rank + 1,
                similarity: candidate.similarity,
                judge: None,
                found_by: candidate.found_by,
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
