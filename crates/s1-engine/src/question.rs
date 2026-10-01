use serde_json::{Map, Value};

use crate::error::EngineError;
use crate::python_json::PythonJson;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Choice,
    Score,
    Noul,
}

impl QuestionKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "choice" => Some(Self::Choice),
            "score" => Some(Self::Score),
            "noul" => Some(Self::Noul),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Choice => "choice",
            Self::Score => "score",
            Self::Noul => "noul",
        }
    }

    /// Index of the type embedding the decision head adds to every token.
    pub fn index(self) -> usize {
        match self {
            Self::Choice => 0,
            Self::Score => 1,
            Self::Noul => 2,
        }
    }
}

/// One answer the model can give: `key` identifies it in the result, `text` is what the model reads.
#[derive(Debug, Clone, PartialEq)]
pub struct AnswerOption {
    pub key: String,
    pub text: String,
}

/// A validated question in the Jev format, with its options already rendered as Laya renders them.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub kind: QuestionKind,
    pub instructions: String,
    pub options: Vec<AnswerOption>,
}

impl Question {
    const NOUL_FALSE_DEFAULT: &'static str = "no, the statement does not hold";
    const NOUL_TRUE_DEFAULT: &'static str = "yes, the statement holds";

    pub fn noul(instructions: impl Into<String>) -> Self {
        Self::from_json(
            "noul",
            &serde_json::json!({"type": "noul", "instructions": instructions.into()}),
        )
        .expect("a noul question with only instructions is always valid")
    }

    /// Parses `{"type", "instructions", "criteria"?, "labels"?}` with the same rules as Laya.
    pub fn from_json(id: &str, definition: &Value) -> Result<Self, EngineError> {
        let invalid = |reason: &str| EngineError::InvalidQuestion {
            id: id.to_string(),
            reason: reason.to_string(),
        };
        let fields = definition
            .as_object()
            .ok_or_else(|| invalid("definition must be an object"))?;
        let kind_name = fields.get("type").and_then(Value::as_str).unwrap_or_default();
        let kind = QuestionKind::parse(kind_name).ok_or_else(|| invalid("type must be choice, score or noul"))?;
        let instructions = match fields.get("instructions") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => PythonJson::dumps(other),
            None => return Err(invalid("no 'instructions'; add the text the model should answer")),
        };
        if kind != QuestionKind::Noul && fields.contains_key("labels") {
            return Err(invalid("labels is only supported for noul questions"));
        }
        let criteria = fields.get("criteria").unwrap_or(&Value::Null);
        let options = match kind {
            QuestionKind::Choice => Self::choice_options(criteria).map_err(|reason| invalid(&reason))?,
            QuestionKind::Score => Self::score_options(criteria).map_err(|reason| invalid(&reason))?,
            QuestionKind::Noul => {
                Self::noul_options(criteria, fields.get("labels")).map_err(|reason| invalid(&reason))?
            }
        };
        Ok(Self {
            kind,
            instructions,
            options,
        })
    }

    fn is_empty_description(value: &Value) -> bool {
        matches!(value, Value::Null) || value.as_str() == Some("")
    }

    fn choice_options(criteria: &Value) -> Result<Vec<AnswerOption>, String> {
        let options: Vec<AnswerOption> = match criteria {
            Value::Array(labels) => labels
                .iter()
                .map(|label| {
                    label
                        .as_str()
                        .map(|key| AnswerOption {
                            key: key.to_string(),
                            text: key.to_string(),
                        })
                        .ok_or_else(|| "choice labels must be strings".to_string())
                })
                .collect::<Result<_, _>>()?,
            Value::Object(described) => described
                .iter()
                .map(|(key, description)| AnswerOption {
                    key: key.clone(),
                    text: if Self::is_empty_description(description) {
                        key.clone()
                    } else {
                        format!("{key}: {}", PythonJson::text(description))
                    },
                })
                .collect(),
            _ => {
                return Err(
                    "a choice question takes 'criteria' as an object of label -> description, or a list of labels"
                        .into(),
                );
            }
        };
        if options.is_empty() {
            return Err("a choice question needs at least one criterion".into());
        }
        Ok(options)
    }

    fn score_options(criteria: &Value) -> Result<Vec<AnswerOption>, String> {
        let levels = criteria.as_array().ok_or_else(|| {
            "a score question takes 'criteria' as a list of level descriptions, index 0 first".to_string()
        })?;
        if levels.is_empty() {
            return Err("a score question needs at least one level".into());
        }
        Ok(levels
            .iter()
            .enumerate()
            .map(|(level, description)| AnswerOption {
                key: level.to_string(),
                text: format!("level {level}: {}", PythonJson::text(description)),
            })
            .collect())
    }

    fn noul_options(criteria: &Value, labels: Option<&Value>) -> Result<Vec<AnswerOption>, String> {
        let descriptions: Map<String, Value> = match criteria {
            Value::Null => Map::new(),
            Value::Object(entries) => entries
                .iter()
                .map(|(key, value)| (key.to_lowercase(), value.clone()))
                .collect(),
            _ => {
                return Err(
                    "a noul question takes 'criteria' as an object with optional 'true'/'false' descriptions".into(),
                );
            }
        };
        if descriptions.keys().any(|key| key != "true" && key != "false") {
            return Err("a noul question takes 'criteria' keyed only 'true'/'false'".into());
        }
        let (false_label, true_label) = Self::noul_labels(labels)?;
        let render = |label: &str, key: &str, default: &str| {
            let description = descriptions.get(key).filter(|value| !Self::is_empty_description(value));
            let text = description.map(PythonJson::text).unwrap_or_else(|| default.to_string());
            AnswerOption {
                key: key.to_string(),
                text: format!("{label}: {text}"),
            }
        };
        Ok(vec![
            render(&false_label, "false", Self::NOUL_FALSE_DEFAULT),
            render(&true_label, "true", Self::NOUL_TRUE_DEFAULT),
        ])
    }

    fn noul_labels(labels: Option<&Value>) -> Result<(String, String), String> {
        let error = || "noul labels must map exactly 'false' and 'true' to distinct non-empty strings".to_string();
        let Some(labels) = labels else {
            return Ok(("false".into(), "true".into()));
        };
        let entries = labels
            .as_object()
            .filter(|entries| entries.len() == 2)
            .ok_or_else(error)?;
        let label = |key: &str| {
            entries
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .map(str::to_string)
        };
        match (label("false"), label("true")) {
            (Some(false_label), Some(true_label))
                if !false_label.is_empty() && !true_label.is_empty() && false_label != true_label =>
            {
                Ok((false_label, true_label))
            }
            _ => Err(error()),
        }
    }
}

/// Questions keyed by id, in the order the caller wrote them.
#[derive(Debug, Clone, Default)]
pub struct QuestionSet {
    entries: Vec<(String, Question)>,
}

impl QuestionSet {
    pub fn from_json(questions: &Value) -> Result<Self, EngineError> {
        let definitions = questions.as_object().ok_or_else(|| EngineError::InvalidQuestion {
            id: String::new(),
            reason: "questions must be an object keyed by question id".into(),
        })?;
        let entries = definitions
            .iter()
            .map(|(id, definition)| Ok((id.clone(), Question::from_json(id, definition)?)))
            .collect::<Result<_, EngineError>>()?;
        Ok(Self { entries })
    }

    pub fn single(id: impl Into<String>, question: Question) -> Self {
        Self {
            entries: vec![(id.into(), question)],
        }
    }

    pub fn push(&mut self, id: impl Into<String>, question: Question) {
        self.entries.push((id.into(), question));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Question)> {
        self.entries.iter().map(|(id, question)| (id.as_str(), question))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Question, QuestionKind};

    fn texts(definition: serde_json::Value) -> Vec<String> {
        Question::from_json("test", &definition)
            .unwrap()
            .options
            .into_iter()
            .map(|option| option.text)
            .collect()
    }

    #[test]
    fn renders_choice_score_and_noul_like_laya() {
        assert_eq!(
            texts(json!({"type": "choice", "instructions": "x", "criteria": ["a", "b"]})),
            ["a", "b"]
        );
        assert_eq!(
            texts(
                json!({"type": "choice", "instructions": "x", "criteria": {"a": "first", "b": "", "c": 0, "d": {"k": 1}}})
            ),
            ["a: first", "b", "c: 0", "d: {\"k\": 1}"]
        );
        assert_eq!(
            texts(json!({"type": "score", "instructions": "x", "criteria": ["low", "high"]})),
            ["level 0: low", "level 1: high"]
        );
        assert_eq!(
            texts(json!({"type": "noul", "instructions": "x"})),
            [
                "false: no, the statement does not hold",
                "true: yes, the statement holds"
            ]
        );
        assert_eq!(
            texts(
                json!({"type": "noul", "instructions": "x", "labels": {"false": " no ", "true": "yes"}, "criteria": {"True": "it is"}})
            ),
            ["no: no, the statement does not hold", "yes: it is"]
        );
    }

    #[test]
    fn rejects_malformed_questions() {
        let rejected = [
            json!({"type": "maybe", "instructions": "x"}),
            json!({"type": "choice"}),
            json!({"type": "choice", "instructions": "x", "criteria": []}),
            json!({"type": "score", "instructions": "x", "criteria": {"a": 1}}),
            json!({"type": "noul", "instructions": "x", "criteria": {"maybe": "y"}}),
            json!({"type": "noul", "instructions": "x", "labels": {"false": "same", "true": "same"}}),
            json!({"type": "choice", "instructions": "x", "criteria": ["a"], "labels": {}}),
        ];
        for definition in rejected {
            assert!(
                Question::from_json("test", &definition).is_err(),
                "accepted {definition}"
            );
        }
    }

    #[test]
    fn noul_shortcut_builds_default_options() {
        let question = Question::noul("It parses JSON.");
        assert_eq!(question.kind, QuestionKind::Noul);
        assert_eq!(question.options.len(), 2);
    }
}
