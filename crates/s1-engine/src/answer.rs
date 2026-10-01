use serde_json::{Map, Value, json};

use crate::question::{Question, QuestionKind};

/// The model's answer to one question: a calibrated distribution over its options.
#[derive(Debug, Clone)]
pub struct Answer {
    pub kind: QuestionKind,
    /// `(option key, probability)` in option order.
    pub probabilities: Vec<(String, f64)>,
    /// Normalized-entropy confidence (`1 - H(p) / ln k`), as Laya reports it for choice and score.
    pub concentration: f64,
    /// Probability of the decision head's "act" output, as opposed to escalating.
    pub act_probability: f64,
}

impl Answer {
    pub fn new(question: &Question, probabilities: Vec<f64>, act_probability: f64) -> Self {
        let concentration = Self::concentration(&probabilities);
        let probabilities = question
            .options
            .iter()
            .map(|option| option.key.clone())
            .zip(probabilities)
            .collect();
        Self {
            kind: question.kind,
            probabilities,
            concentration,
            act_probability,
        }
    }

    fn concentration(probabilities: &[f64]) -> f64 {
        let count = probabilities.len();
        if count < 2 {
            return 1.0;
        }
        let entropy: f64 = probabilities
            .iter()
            .map(|probability| -probability * probability.clamp(1e-12, 1.0).ln())
            .sum();
        (1.0 - entropy / (count as f64).ln()).clamp(0.0, 1.0)
    }

    /// The most likely option key.
    pub fn choice(&self) -> &str {
        self.probabilities
            .iter()
            .fold(None::<&(String, f64)>, |best, entry| match best {
                Some(current) if current.1 >= entry.1 => Some(current),
                _ => Some(entry),
            })
            .map(|(key, _)| key.as_str())
            .unwrap_or_default()
    }

    /// Expected level of a score question.
    pub fn score(&self) -> f64 {
        self.probabilities
            .iter()
            .enumerate()
            .map(|(level, (_, probability))| level as f64 * probability)
            .sum()
    }

    /// P(true) of a noul question.
    pub fn noul(&self) -> f64 {
        self.probabilities
            .get(1)
            .map(|(_, probability)| *probability)
            .unwrap_or_default()
    }

    /// Probability mass on the reported answer (what calibration guarantees).
    pub fn confidence(&self) -> f64 {
        match self.kind {
            QuestionKind::Noul => self.noul().max(1.0 - self.noul()),
            _ => self.concentration,
        }
    }

    /// The Jev / Laya response shape for this answer.
    pub fn to_json(&self) -> Value {
        let probabilities: Map<String, Value> = self
            .probabilities
            .iter()
            .map(|(key, probability)| (key.clone(), json!(probability)))
            .collect();
        let action = json!({"act_probability": self.act_probability});
        match self.kind {
            QuestionKind::Choice => json!({"type": "choice", "choice": self.choice(), "probabilities": probabilities,
                                           "confidence": self.confidence(), "action": action}),
            QuestionKind::Score => json!({"type": "score", "score": self.score(), "probabilities": probabilities,
                                          "confidence": self.confidence(), "action": action}),
            QuestionKind::Noul => {
                json!({"type": "noul", "noul": self.noul(), "confidence": self.confidence(), "action": action})
            }
        }
    }
}

/// Answers keyed by question id, in question order, plus how many tokens the model read.
#[derive(Debug, Clone, Default)]
pub struct Decision {
    pub answers: Vec<(String, Answer)>,
    pub input_tokens: usize,
}

impl Decision {
    pub fn answer(&self, question_id: &str) -> Option<&Answer> {
        self.answers
            .iter()
            .find(|(id, _)| id == question_id)
            .map(|(_, answer)| answer)
    }

    pub fn to_json(&self) -> Value {
        let answers: Map<String, Value> = self
            .answers
            .iter()
            .map(|(id, answer)| (id.clone(), answer.to_json()))
            .collect();
        json!({"answers": answers, "usage": {"input_tokens": self.input_tokens, "output_tokens": 0}})
    }
}
