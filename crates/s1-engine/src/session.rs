use std::path::Path;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use crate::accelerator::Accelerator;
use crate::error::EngineError;
use crate::sequence::EncodedQuestion;

/// Raw outputs for a padded batch: `logits[row][marker]` and `act_logits[row][action]`.
pub struct BatchOutput {
    pub logits: Vec<Vec<f32>>,
    pub act_logits: Vec<Vec<f32>>,
}

/// The exported decision graph running on ONNX Runtime.
pub struct DecisionSession {
    session: Session,
}

impl DecisionSession {
    pub fn load(graph_path: &Path, threads: usize, accelerator: Accelerator) -> Result<Self, EngineError> {
        let builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .and_then(|builder| builder.with_intra_threads(threads))
            .map_err(ort::Error::from)?;
        let session = accelerator.configure(builder)?.commit_from_file(graph_path)?;
        Ok(Self { session })
    }

    /// Pads the batch like Laya's `collate_items` and runs one forward pass.
    pub fn run(&mut self, batch: &[EncodedQuestion], pad_id: u32) -> Result<BatchOutput, EngineError> {
        let rows = batch.len();
        let columns = batch.iter().map(|item| item.input_ids.len()).max().unwrap_or(0);
        let marker_columns = batch.iter().map(|item| item.markers.len()).max().unwrap_or(0);
        let mut input_ids = vec![i64::from(pad_id); rows * columns];
        let mut attention_mask = vec![0_i64; rows * columns];
        let mut marker_positions = vec![0_i64; rows * marker_columns];
        let mut marker_mask = vec![false; rows * marker_columns];
        let mut kind_indexes = Vec::with_capacity(rows);
        for (row, item) in batch.iter().enumerate() {
            for (column, token) in item.input_ids.iter().enumerate() {
                input_ids[row * columns + column] = i64::from(*token);
                attention_mask[row * columns + column] = 1;
            }
            for (column, position) in item.markers.iter().enumerate() {
                marker_positions[row * marker_columns + column] = *position as i64;
                marker_mask[row * marker_columns + column] = true;
            }
            kind_indexes.push(item.kind_index as i64);
        }
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Tensor::from_array(([rows, columns], input_ids))?,
            "attention_mask" => Tensor::from_array(([rows, columns], attention_mask))?,
            "marker_pos" => Tensor::from_array(([rows, marker_columns], marker_positions))?,
            "marker_mask" => Tensor::from_array(([rows, marker_columns], marker_mask))?,
            "qtype" => Tensor::from_array(([rows], kind_indexes))?,
        ])?;
        let logits = Self::rows(&outputs["logits"], rows)?;
        let act_logits = Self::rows(&outputs["act_logits"], rows)?;
        Ok(BatchOutput { logits, act_logits })
    }

    fn rows(value: &ort::value::DynValue, rows: usize) -> Result<Vec<Vec<f32>>, EngineError> {
        let (shape, data) = value.try_extract_tensor::<f32>()?;
        if shape.len() != 2 || shape[0] as usize != rows {
            return Err(EngineError::UnexpectedOutput(format!(
                "expected {rows} rows, got shape {shape:?}"
            )));
        }
        let width = shape[1] as usize;
        Ok(data.chunks(width.max(1)).take(rows).map(<[f32]>::to_vec).collect())
    }
}
