use crate::settings::{BundleFiles, EmbedderSettings};
use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde::Deserialize;
use tokenizers::{Tokenizer, TruncationParams};

use crate::accelerator::Accelerator;
use crate::error::EngineError;

/// How a sequence of token vectors becomes one text vector.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Pooling {
    /// The first token (granite, BERT-style encoders).
    Cls,
    /// The last real token (Qwen3-Embedding and other decoder embedders).
    Last,
    /// The average of all real tokens.
    Mean,
}

/// `embedder_config.json`, written by `model_export export-embedder`.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbedderConfig {
    pub pooling: Pooling,
    #[serde(default = "EmbedderConfig::default_normalize")]
    pub normalize: bool,
    pub max_tokens: usize,
    pub dimension: usize,
    #[serde(default)]
    pub query_prompt: String,
    #[serde(default)]
    pub document_prompt: String,
    #[serde(default)]
    pub pad_token: Option<String>,
}

impl EmbedderConfig {
    fn default_normalize() -> bool {
        true
    }
}

/// A sentence-embedding model directory: `model.onnx` (returns `last_hidden_state`), `tokenizer.json` and
/// `embedder_config.json`.
#[derive(Debug, Clone)]
pub struct EmbedderBundle {
    pub directory: PathBuf,
    pub config: EmbedderConfig,
}

impl EmbedderBundle {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, EngineError> {
        let directory = directory.into();
        for file in [BundleFiles::GRAPH, BundleFiles::TOKENIZER, BundleFiles::EMBEDDER_CONFIG] {
            if !directory.join(file).is_file() {
                return Err(EngineError::MissingFile(directory.join(file)));
            }
        }
        let path = directory.join(BundleFiles::EMBEDDER_CONFIG);
        let text = std::fs::read_to_string(&path).map_err(|source| EngineError::Io {
            path: path.clone(),
            source,
        })?;
        let config = serde_json::from_str(&text).map_err(|source| EngineError::InvalidJson { path, source })?;
        Ok(Self { directory, config })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Turns texts into unit vectors with an exported embedding model, matching sentence-transformers.
pub struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
    config: EmbedderConfig,
    pad_id: u32,
    batch_size: usize,
}

impl Embedder {
    pub fn load(bundle: &EmbedderBundle, threads: usize, accelerator: Accelerator) -> Result<Self, EngineError> {
        let builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .and_then(|builder| builder.with_intra_threads(threads))
            .map_err(ort::Error::from)?;
        let session = accelerator
            .configure(builder)?
            .commit_from_file(bundle.directory.join(BundleFiles::GRAPH))?;
        let mut tokenizer = Tokenizer::from_file(bundle.directory.join(BundleFiles::TOKENIZER))
            .map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: bundle.config.max_tokens,
                ..Default::default()
            }))
            .map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        tokenizer.with_padding(None);
        let pad_id = bundle
            .config
            .pad_token
            .iter()
            .map(String::as_str)
            .chain(EmbedderSettings::PAD_CANDIDATES)
            .find_map(|token| tokenizer.token_to_id(token))
            .ok_or_else(|| EngineError::MissingSpecialToken("<pad>".to_string()))?;
        Ok(Self {
            session,
            tokenizer,
            config: bundle.config.clone(),
            pad_id,
            batch_size: EmbedderSettings::BATCH,
        })
    }

    pub fn config(&self) -> &EmbedderConfig {
        &self.config
    }

    /// Vectors for code units, in the order given.
    pub fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, EngineError> {
        let prompt = self.config.document_prompt.clone();
        let prefixed: Vec<String> = texts.iter().map(|text| format!("{prompt}{text}")).collect();
        self.embed(&prefixed)
    }

    /// The vector for one search, with the model's query instruction in front.
    pub fn embed_query(&mut self, query: &str) -> Result<Vec<f32>, EngineError> {
        let text = format!("{}{query}", self.config.query_prompt);
        Ok(self.embed(&[text])?.remove(0))
    }

    /// Embeds texts exactly as given, batching texts of similar length together and restoring the order.
    pub fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, EngineError> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        let token_lists: Vec<Vec<u32>> = encodings.iter().map(|encoding| encoding.get_ids().to_vec()).collect();
        let mut order: Vec<usize> = (0..token_lists.len()).collect();
        order.sort_by_key(|&index| token_lists[index].len());
        let mut vectors = vec![Vec::new(); token_lists.len()];
        for chunk in order.chunks(self.batch_size) {
            let batch: Vec<&[u32]> = chunk.iter().map(|&index| token_lists[index].as_slice()).collect();
            for (&index, vector) in chunk.iter().zip(self.run(&batch)?) {
                vectors[index] = vector;
            }
        }
        Ok(vectors)
    }

    fn run(&mut self, batch: &[&[u32]]) -> Result<Vec<Vec<f32>>, EngineError> {
        let rows = batch.len();
        let columns = batch.iter().map(|tokens| tokens.len()).max().unwrap_or(1).max(1);
        let left = self.config.pooling == Pooling::Last;
        let mut input_ids = vec![i64::from(self.pad_id); rows * columns];
        let mut attention_mask = vec![0_i64; rows * columns];
        for (row, tokens) in batch.iter().enumerate() {
            let offset = if left { columns - tokens.len() } else { 0 };
            for (column, token) in tokens.iter().enumerate() {
                input_ids[row * columns + offset + column] = i64::from(*token);
                attention_mask[row * columns + offset + column] = 1;
            }
        }
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Tensor::from_array(([rows, columns], input_ids))?,
            "attention_mask" => Tensor::from_array(([rows, columns], attention_mask.clone()))?,
        ])?;
        let (shape, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        if shape.len() != 3 || shape[0] as usize != rows || shape[1] as usize != columns {
            return Err(EngineError::UnexpectedOutput(format!(
                "last_hidden_state has shape {shape:?}"
            )));
        }
        let width = shape[2] as usize;
        let mut vectors = Vec::with_capacity(rows);
        for row in 0..rows {
            let token = |column: usize| &data[(row * columns + column) * width..(row * columns + column + 1) * width];
            let real: Vec<usize> = (0..columns)
                .filter(|&column| attention_mask[row * columns + column] == 1)
                .collect();
            let mut vector = match self.config.pooling {
                Pooling::Cls => token(real.first().copied().unwrap_or(0)).to_vec(),
                Pooling::Last => token(real.last().copied().unwrap_or(columns - 1)).to_vec(),
                Pooling::Mean => {
                    let mut sum = vec![0.0_f32; width];
                    for &column in &real {
                        for (total, value) in sum.iter_mut().zip(token(column)) {
                            *total += value;
                        }
                    }
                    sum.iter().map(|value| value / real.len().max(1) as f32).collect()
                }
            };
            if self.config.normalize {
                let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt().max(1e-12);
                vector.iter_mut().for_each(|value| *value /= norm);
            }
            vectors.push(vector);
        }
        Ok(vectors)
    }
}
