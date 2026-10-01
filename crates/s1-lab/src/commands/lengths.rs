use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use s1_index::{ExtractorRegistry, SourceWalker};
use s1grep::models::{ModelDirectory, Retriever};
use s1grep::service::FileFilters;
use tokenizers::Tokenizer;

#[derive(Args)]
pub struct LengthsCommand {
    /// Repositories to measure (repeatable)
    #[arg(required = true)]
    repositories: Vec<PathBuf>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl LengthsCommand {
    /// Token lengths of the text the retriever embeds for each function, and how much of the embedding work each
    /// token cap would leave (cost grows about linearly with tokens).
    pub fn run(self) -> anyhow::Result<()> {
        let bundle = self.models.bundle(Retriever::Granite.bundle_name())?;
        let tokenizer = Tokenizer::from_file(bundle.join("tokenizer.json"))
            .map_err(|error| anyhow::anyhow!("{error}"))
            .context("loading the retriever tokenizer")?;
        let mut extractors = ExtractorRegistry::new()?;
        let options = FileFilters::default().walk_options()?;
        let mut lengths = Vec::new();
        for repository in &self.repositories {
            for file in SourceWalker::new(repository, options.clone()).files()? {
                let Ok(bytes) = std::fs::read(&file.absolute) else {
                    continue;
                };
                for unit in extractors.extract(&file.relative, &String::from_utf8_lossy(&bytes)) {
                    let encoding = tokenizer
                        .encode(unit.document_text(), true)
                        .map_err(|error| anyhow::anyhow!("{error}"))?;
                    lengths.push(encoding.get_ids().len());
                }
            }
        }
        lengths.sort_unstable();
        let count = lengths.len().max(1);
        let percentile = |share: f64| lengths[((count as f64 * share) as usize).min(count - 1)];
        println!("functions {}", lengths.len());
        println!(
            "tokens    mean {:.0}  p50 {}  p75 {}  p90 {}  p99 {}",
            lengths.iter().sum::<usize>() as f64 / count as f64,
            percentile(0.5),
            percentile(0.75),
            percentile(0.9),
            percentile(0.99)
        );
        let work_at = |cap: usize| lengths.iter().map(|length| (*length).min(cap)).sum::<usize>() as f64;
        let full = work_at(512);
        for cap in [512, 384, 256, 192, 128] {
            let truncated = lengths.iter().filter(|length| **length > cap).count();
            println!(
                "cap {cap:>3}  work {:>5.1} %  functions cut {:>5.1} %",
                work_at(cap) * 100.0 / full,
                truncated as f64 * 100.0 / count as f64
            );
        }
        Ok(())
    }
}
