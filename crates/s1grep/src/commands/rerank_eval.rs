use std::io::BufRead;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::Args;
use s1_engine::{EngineOptions, LayaEngine, ModelBundle, QuestionSet};
use s1_index::{FusionWeights, RankFusion};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::models::ModelDirectory;

/// One question with its retriever candidates already chosen, best first, and the index of the right one (none when the retriever missed it).
#[derive(Deserialize)]
struct RankedQuestion {
    text: String,
    language: String,
    gold: Option<usize>,
    candidates: Vec<String>,
}

impl RankedQuestion {
    /// The candidate as the judge reads it: path, name, empty line, then the source cut like `CodeUnit::judge_state`.
    fn judge_state(candidate: &str) -> String {
        match candidate.split_once("\n\n") {
            Some((header, source)) => {
                let cut: String = source
                    .chars()
                    .take(s1_index::CodeUnit::JUDGE_SOURCE_CHARACTERS)
                    .collect();
                format!("{header}\n\n{cut}")
            }
            None => candidate.to_string(),
        }
    }
}

#[derive(Args)]
pub struct RerankEvalCommand {
    /// JSON lines with `text`, `language`, `gold` and `candidates` (retriever order)
    #[arg(long)]
    questions: PathBuf,
    /// Candidate counts the judge reads, comma separated
    #[arg(long, value_delimiter = ',', default_value = "5,10")]
    judged: Vec<usize>,
    /// Sequence lengths to try, comma separated (the model was trained with 384)
    #[arg(long, value_delimiter = ',', default_value = "384")]
    max_len: Vec<usize>,
    /// Candidates per ONNX Runtime run (0: all judged candidates in one run)
    #[arg(long, default_value_t = 0)]
    batch: usize,
    #[arg(long)]
    threads: Option<usize>,
    /// Only the first N questions
    #[arg(long)]
    limit: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl RerankEvalCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let file =
            std::fs::File::open(&self.questions).with_context(|| format!("reading {}", self.questions.display()))?;
        let mut questions = Vec::new();
        for line in std::io::BufReader::new(file).lines() {
            let line = line?;
            if !line.trim().is_empty() {
                questions.push(serde_json::from_str::<RankedQuestion>(&line)?);
            }
        }
        if let Some(limit) = self.limit {
            questions.truncate(limit);
        }
        let bundle = ModelBundle::open(self.models.bundle(crate::settings::ModelSettings::JUDGE_BUNDLE)?)?;
        let mut rows = Vec::new();
        for &max_len in &self.max_len {
            let mut options = EngineOptions {
                max_len: Some(max_len),
                ..EngineOptions::default()
            };
            if let Some(threads) = self.threads {
                options.threads = threads;
            }
            let mut judge = LayaEngine::load(&bundle, &options)?;
            for &judged in &self.judged {
                rows.push(self.measure(&mut judge, &questions, judged, max_len)?);
                eprintln!("{}", rows.last().unwrap());
            }
        }
        println!("{}", serde_json::to_string_pretty(&rows)?);
        Ok(())
    }

    fn measure(
        &self,
        judge: &mut LayaEngine,
        questions: &[RankedQuestion],
        judged: usize,
        max_len: usize,
    ) -> anyhow::Result<Value> {
        let weights = FusionWeights::tuned("granite", judged);
        let mut seconds = Vec::with_capacity(questions.len());
        let mut hits = serde_json::Map::new();
        let mut tally = |key: &str, field: &str, hit: bool| {
            let entry = hits
                .entry(key.to_string())
                .or_insert_with(|| json!({"questions": 0, "retriever_top1": 0, "judge_top1": 0, "fused_top1": 0}));
            entry[field] = json!(entry[field].as_u64().unwrap() + u64::from(hit));
        };
        for question in questions {
            let count = judged.min(question.candidates.len());
            let states: Vec<Value> = question.candidates[..count]
                .iter()
                .map(|candidate| Value::String(RankedQuestion::judge_state(candidate)))
                .collect();
            let started = Instant::now();
            let asked = QuestionSet::from_json(&json!({
                "answers": {"type": "noul", "instructions": format!("{}{}", crate::settings::SearchSettings::QUESTION_TEMPLATE, question.text)}
            }))?;
            let chunk = if self.batch == 0 { count.max(1) } else { self.batch };
            let mut scores = Vec::with_capacity(count);
            for part in states.chunks(chunk) {
                scores.extend(
                    judge
                        .decide_batch(part, &asked)?
                        .iter()
                        .map(|decision| decision.answers[0].1.noul()),
                );
            }
            let order = RankFusion::order(question.candidates.len(), &scores, weights);
            seconds.push(started.elapsed().as_secs_f64());
            let judge_best =
                (0..scores.len()).max_by(|&left, &right| scores[left].total_cmp(&scores[right]).then(right.cmp(&left)));
            for key in ["all", question.language.as_str()] {
                tally(key, "questions", true);
                tally(key, "retriever_top1", question.gold == Some(0));
                tally(key, "judge_top1", judge_best.is_some() && judge_best == question.gold);
                tally(key, "fused_top1", order.first().copied() == question.gold);
            }
        }
        seconds.sort_by(f64::total_cmp);
        let median = seconds[seconds.len() / 2];
        Ok(
            json!({"max_len": max_len, "judged": judged, "batch": self.batch, "median_seconds": (median * 1000.0).round() / 1000.0,
                  "results": hits}),
        )
    }
}
