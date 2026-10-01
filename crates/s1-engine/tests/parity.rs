//! Checks the Rust engine against outputs recorded from Laya's Python runtime
//! (`./dev.sh export python -m model_export fixtures [--source <checkpoint> --name <bundle>]`).
//! Every bundle under `models/` that has a recorded fixture is checked: the base Laya model and s1-code.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{Map, Value};

use s1_engine::{EngineOptions, LayaEngine, ModelBundle, QuestionSet};

const PROBABILITY_TOLERANCE: f64 = 6e-4;

#[derive(Deserialize)]
struct ParityFixture {
    cases: Vec<ParityCase>,
}

#[derive(Deserialize)]
struct ParityCase {
    name: String,
    state: Value,
    questions: Value,
    sequences: Map<String, Value>,
    answers: Map<String, Value>,
}

struct ParityHarness {
    name: &'static str,
    fixture: ParityFixture,
    engine: Mutex<LayaEngine>,
}

impl ParityHarness {
    const BUNDLES: [&'static str; 2] = ["laya-multilingual", "s1-code-v3-onnx"];

    fn shared() -> &'static [ParityHarness] {
        static HARNESSES: OnceLock<Vec<ParityHarness>> = OnceLock::new();
        HARNESSES.get_or_init(|| Self::BUNDLES.iter().filter_map(|name| Self::load(name)).collect())
    }

    fn load(name: &'static str) -> Option<Self> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let model_directory = manifest.join("../../models").join(name);
        let fixture_path = manifest.join("tests/fixtures").join(format!("{name}.json"));
        if !model_directory.join(s1_engine::BundleFiles::GRAPH).is_file() || !fixture_path.is_file() {
            eprintln!("skipping parity for {name}: no bundle or fixture");
            return None;
        }
        let fixture: ParityFixture =
            serde_json::from_str(&std::fs::read_to_string(fixture_path).expect("fixture file")).expect("fixture JSON");
        let bundle = ModelBundle::open(model_directory).expect("model bundle");
        let engine = LayaEngine::load(&bundle, &EngineOptions::default()).expect("engine");
        Some(Self {
            name,
            fixture,
            engine: Mutex::new(engine),
        })
    }

    fn close(actual: f64, expected: &Value) -> bool {
        (actual - expected.as_f64().expect("numeric expectation")).abs() <= PROBABILITY_TOLERANCE
    }
}

#[test]
fn sequences_match_python_token_for_token() {
    for harness in ParityHarness::shared() {
        let engine = harness.engine.lock().unwrap();
        for case in &harness.fixture.cases {
            let questions = QuestionSet::from_json(&case.questions).expect("questions");
            let encoded = engine.encode(&case.state, &questions).expect("encode");
            for ((question_id, _), sequence) in questions.iter().zip(&encoded) {
                let expected = &case.sequences[question_id];
                let expected_ids: Vec<u32> = serde_json::from_value(expected["input_ids"].clone()).unwrap();
                let expected_markers: Vec<usize> = serde_json::from_value(expected["markers"].clone()).unwrap();
                assert_eq!(
                    sequence.input_ids, expected_ids,
                    "{} / {question_id}: input ids differ",
                    case.name
                );
                assert_eq!(
                    sequence.markers, expected_markers,
                    "{} {} / {question_id}: markers differ",
                    harness.name, case.name
                );
            }
        }
    }
}

#[test]
fn answers_match_python_onnx_runtime() {
    for harness in ParityHarness::shared() {
        let mut engine = harness.engine.lock().unwrap();
        for case in &harness.fixture.cases {
            let questions = QuestionSet::from_json(&case.questions).expect("questions");
            let decision = engine.decide(&case.state, &questions).expect("decide");
            for (question_id, answer) in &decision.answers {
                let label = format!("{} {} / {question_id}", harness.name, case.name);
                let expected = &case.answers["onnx"][question_id.as_str()];
                match expected["type"].as_str().unwrap() {
                    "noul" => assert!(
                        ParityHarness::close(answer.noul(), &expected["noul"]),
                        "{label}: noul {} vs {}",
                        answer.noul(),
                        expected["noul"]
                    ),
                    kind => {
                        for (key, probability) in &answer.probabilities {
                            let reference = &expected["probabilities"][key.as_str()];
                            assert!(
                                ParityHarness::close(*probability, reference),
                                "{label}: P({key}) {probability} vs {reference}"
                            );
                        }
                        if kind == "choice" {
                            assert_eq!(answer.choice(), expected["choice"].as_str().unwrap(), "{label}: choice");
                        } else {
                            assert!(
                                ParityHarness::close(answer.score(), &expected["score"]),
                                "{label}: score"
                            );
                        }
                    }
                }
                assert!(
                    ParityHarness::close(answer.confidence(), &expected["confidence"]),
                    "{label}: confidence"
                );
                assert!(
                    ParityHarness::close(answer.act_probability, &expected["action"]["act_probability"]),
                    "{label}: act probability"
                );
            }
        }
    }
}
