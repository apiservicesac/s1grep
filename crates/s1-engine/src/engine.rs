use serde_json::Value;

use crate::accelerator::Accelerator;
use crate::answer::{Answer, Decision};
use crate::bundle::ModelBundle;
use crate::calibration::Temperatures;
use crate::error::EngineError;
use crate::python_json::PythonJson;
use crate::question::QuestionSet;
use crate::sequence::{EncodedQuestion, SequenceBuilder};
use crate::session::DecisionSession;
use crate::tokenizer::LayaTokenizer;

#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// ONNX Runtime intra-op threads. Physical cores work best; hyper-threads and efficiency cores slow it down.
    pub threads: usize,
    pub accelerator: Accelerator,
    pub max_len: Option<usize>,
    pub head_max_len: Option<usize>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        let logical = std::thread::available_parallelism().map(usize::from).unwrap_or(2);
        Self {
            threads: (logical / 2).max(1),
            accelerator: Accelerator::Cpu,
            max_len: None,
            head_max_len: None,
        }
    }
}

/// Laya running locally: encodes questions, runs the graph and turns logits into calibrated answers.
pub struct LayaEngine {
    tokenizer: LayaTokenizer,
    builder: SequenceBuilder,
    temperatures: Temperatures,
    session: DecisionSession,
}

impl LayaEngine {
    pub fn load(bundle: &ModelBundle, options: &EngineOptions) -> Result<Self, EngineError> {
        let tokenizer = LayaTokenizer::load(&bundle.tokenizer_path(), &bundle.tokenizer_config_path())?;
        let builder = SequenceBuilder {
            max_len: options.max_len.unwrap_or(bundle.config.max_len),
            head_max_len: options.head_max_len.unwrap_or(bundle.config.head_max_len),
        };
        let temperatures = Temperatures::from_config(&bundle.config);
        let session = DecisionSession::load(&bundle.graph_path(), options.threads, options.accelerator)?;
        Ok(Self {
            tokenizer,
            builder,
            temperatures,
            session,
        })
    }

    pub fn tokenizer(&self) -> &LayaTokenizer {
        &self.tokenizer
    }

    pub fn sequence_builder(&self) -> &SequenceBuilder {
        &self.builder
    }

    /// Encodes every question about `state`, tokenizing the state once.
    pub fn encode(&self, state: &Value, questions: &QuestionSet) -> Result<Vec<EncodedQuestion>, EngineError> {
        let state_ids = self
            .tokenizer
            .encode(&self.tokenizer.neutralize(&PythonJson::text(state)))?;
        let truncate_left = state.is_array();
        questions
            .iter()
            .map(|(id, question)| {
                self.builder
                    .build(&self.tokenizer, id, question, &state_ids, truncate_left)
            })
            .collect()
    }

    /// Answers every question about one state in a single forward pass.
    pub fn decide(&mut self, state: &Value, questions: &QuestionSet) -> Result<Decision, EngineError> {
        if questions.is_empty() {
            return Ok(Decision::default());
        }
        let encoded = self.encode(state, questions)?;
        let output = self.session.run(&encoded, self.tokenizer.pad_id)?;
        let mut answers = Vec::with_capacity(questions.len());
        for (row, (id, question)) in questions.iter().enumerate() {
            let option_count = encoded[row].markers.len();
            let temperature = self.temperatures.for_question(question.kind, option_count);
            let probabilities = Self::softmax(&output.logits[row][..option_count], temperature);
            let act_probability = Self::softmax(&output.act_logits[row], 1.0)
                .first()
                .copied()
                .unwrap_or_default();
            answers.push((id.to_string(), Answer::new(question, probabilities, act_probability)));
        }
        let input_tokens = encoded.iter().map(|item| item.input_ids.len()).sum();
        Ok(Decision { answers, input_tokens })
    }

    /// Answers the same questions about several states in one forward pass (used to judge candidates together).
    pub fn decide_batch(&mut self, states: &[Value], questions: &QuestionSet) -> Result<Vec<Decision>, EngineError> {
        if questions.is_empty() || states.is_empty() {
            return Ok(states.iter().map(|_| Decision::default()).collect());
        }
        let mut encoded = Vec::with_capacity(states.len() * questions.len());
        for state in states {
            encoded.extend(self.encode(state, questions)?);
        }
        let output = self.session.run(&encoded, self.tokenizer.pad_id)?;
        let mut decisions = Vec::with_capacity(states.len());
        for (state_index, rows) in encoded.chunks(questions.len()).enumerate() {
            let mut answers = Vec::with_capacity(questions.len());
            for (offset, (id, question)) in questions.iter().enumerate() {
                let row = state_index * questions.len() + offset;
                let option_count = rows[offset].markers.len();
                let temperature = self.temperatures.for_question(question.kind, option_count);
                let probabilities = Self::softmax(&output.logits[row][..option_count], temperature);
                let act_probability = Self::softmax(&output.act_logits[row], 1.0)
                    .first()
                    .copied()
                    .unwrap_or_default();
                answers.push((id.to_string(), Answer::new(question, probabilities, act_probability)));
            }
            let input_tokens = rows.iter().map(|item| item.input_ids.len()).sum();
            decisions.push(Decision { answers, input_tokens });
        }
        Ok(decisions)
    }

    fn softmax(logits: &[f32], temperature: f64) -> Vec<f64> {
        let scaled: Vec<f64> = logits.iter().map(|logit| f64::from(*logit) / temperature).collect();
        let highest = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let exponentials: Vec<f64> = scaled.iter().map(|value| (value - highest).exp()).collect();
        let total: f64 = exponentials.iter().sum();
        exponentials.into_iter().map(|value| value / total).collect()
    }
}
