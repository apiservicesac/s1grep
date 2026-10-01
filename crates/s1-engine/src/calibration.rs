use crate::settings::CalibrationLimits;
use std::collections::HashMap;

use serde_json::Value;

use crate::bundle::DecisionConfig;
use crate::question::QuestionKind;

/// Fitted temperatures per question type and option count, clamped the way Laya clamps them:
/// a temperature below 0.5 would turn a coin flip into a confident answer.
#[derive(Debug, Clone)]
pub struct Temperatures {
    by_kind: [f64; 3],
    by_bucket: HashMap<String, f64>,
}

impl Temperatures {
    pub fn from_config(config: &DecisionConfig) -> Self {
        let mut by_kind = [1.0; 3];
        for (slot, value) in by_kind.iter_mut().zip(&config.temperature) {
            *slot = Self::clamp(value);
        }
        let by_bucket = config
            .temperature_by_options
            .iter()
            .map(|(bucket, value)| (bucket.clone(), Self::clamp(value)))
            .collect();
        Self { by_kind, by_bucket }
    }

    pub fn for_question(&self, kind: QuestionKind, option_count: usize) -> f64 {
        self.by_bucket
            .get(&Self::bucket(kind, option_count))
            .copied()
            .unwrap_or(self.by_kind[kind.index()])
    }

    fn bucket(kind: QuestionKind, option_count: usize) -> String {
        let size = match option_count {
            0..=2 => "2",
            3..=5 => "3-5",
            6..=10 => "6-10",
            _ => "11+",
        };
        format!("{}:{size}", kind.name())
    }

    fn clamp(value: &Value) -> f64 {
        let number = match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        };
        match number {
            Some(number) if number.is_finite() => number.clamp(
                CalibrationLimits::MINIMUM_TEMPERATURE,
                CalibrationLimits::MAXIMUM_TEMPERATURE,
            ),
            _ => 1.0,
        }
    }
}
