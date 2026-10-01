use std::io::Read;
use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use s1_engine::{Accelerator, EngineOptions, LayaEngine, QuestionSet};
use serde_json::Value;

use crate::model_locator::ModelLocator;

#[derive(Args)]
pub struct DecideCommand {
    /// Questions as a JSON object keyed by id, in the Jev format, or @file.json
    #[arg(long)]
    questions: String,
    /// File with the state; reads stdin when omitted. JSON files are parsed, anything else is text
    state: Option<PathBuf>,
    #[arg(long)]
    threads: Option<usize>,
    /// Where the model runs: cpu, directml, openvino-cpu or openvino-gpu
    #[arg(long, default_value_t = Accelerator::Cpu)]
    device: Accelerator,
    #[command(flatten)]
    model: ModelLocator,
}

impl DecideCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let questions_text = match self.questions.strip_prefix('@') {
            Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?,
            None => self.questions.clone(),
        };
        let questions: Value = serde_json::from_str(&questions_text).context("--questions is not valid JSON")?;
        let questions = QuestionSet::from_json(&questions)?;
        let state = self.read_state()?;
        let mut options = EngineOptions {
            accelerator: self.device,
            ..EngineOptions::default()
        };
        if let Some(threads) = self.threads {
            options.threads = threads;
        }
        let mut engine = LayaEngine::load(&self.model.open()?, &options)?;
        let decision = engine.decide(&state, &questions)?;
        println!("{}", serde_json::to_string_pretty(&decision.to_json())?);
        Ok(())
    }

    fn read_state(&self) -> anyhow::Result<Value> {
        let text = match &self.state {
            Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
            None => {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                text
            }
        };
        let is_json = self
            .state
            .as_ref()
            .is_some_and(|path| path.extension().is_some_and(|extension| extension == "json"));
        Ok(if is_json {
            serde_json::from_str(&text)?
        } else {
            Value::String(text)
        })
    }
}
