//! Checks the Rust embedder against sentence-transformers vectors recorded by
//! `./dev.sh export python -m model_export embedder-fixtures --embedder <name>`.

use std::path::PathBuf;

use serde::Deserialize;

use s1_engine::{Accelerator, Embedder, EmbedderBundle};

const MINIMUM_COSINE: f32 = 0.999;
const BUNDLES: [&str; 3] = ["granite-278m-onnx", "granite-97m-r2-onnx", "qwen3-embedding-0.6b-onnx"];

#[derive(Deserialize)]
struct EmbedderFixture {
    texts: Vec<String>,
    vectors: Vec<Vec<f32>>,
}

#[test]
fn vectors_match_sentence_transformers() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for name in BUNDLES {
        let directory = manifest.join("../../models").join(name);
        let fixture_path = manifest.join("tests/fixtures").join(format!("{name}.json"));
        if !directory.join(s1_engine::BundleFiles::GRAPH).is_file() || !fixture_path.is_file() {
            eprintln!("skipping embedder parity for {name}: no bundle or fixture");
            continue;
        }
        let fixture: EmbedderFixture =
            serde_json::from_str(&std::fs::read_to_string(fixture_path).unwrap()).expect("fixture JSON");
        let bundle = EmbedderBundle::open(&directory).expect("bundle");
        let mut embedder = Embedder::load(&bundle, 4, Accelerator::Cpu).expect("embedder");
        let actual = embedder.embed(&fixture.texts).expect("embed");
        for ((text, expected), vector) in fixture.texts.iter().zip(&fixture.vectors).zip(&actual) {
            assert_eq!(vector.len(), expected.len(), "{name}: dimension");
            let cosine: f32 = vector.iter().zip(expected).map(|(a, b)| a * b).sum();
            let preview: String = text.chars().take(40).collect();
            assert!(cosine >= MINIMUM_COSINE, "{name}: cosine {cosine} for {preview:?}");
        }
        eprintln!("{name}: {} vectors match", actual.len());
    }
}
