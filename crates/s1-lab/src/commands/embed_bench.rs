use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions};
use s1_index::{ExtractorRegistry, SourceWalker};
use s1grep::models::{ModelDirectory, Retriever};
use s1grep::service::FileFilters;

#[derive(Args)]
pub struct EmbedBenchCommand {
    /// Repository whose functions are embedded
    repository: PathBuf,
    /// Functions to embed with each retriever
    #[arg(long, default_value_t = 500)]
    functions: usize,
    /// Retrievers to compare
    #[arg(long, value_enum, value_delimiter = ',', default_values = ["granite", "granite-small"])]
    retrievers: Vec<Retriever>,
    /// Intra-op thread counts to compare (default: half the logical CPUs)
    #[arg(long, value_delimiter = ',')]
    threads: Vec<usize>,
    /// Texts per run to compare (default: the shipped batch)
    #[arg(long, value_delimiter = ',')]
    batches: Vec<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl EmbedBenchCommand {
    /// Embeds the same functions with each retriever and reports functions per second, whole sources and outlines.
    pub fn run(self) -> anyhow::Result<()> {
        let mut extractors = ExtractorRegistry::new()?;
        let mut units = Vec::new();
        for file in SourceWalker::new(&self.repository, FileFilters::default().walk_options()?).files()? {
            let Ok(bytes) = std::fs::read(&file.absolute) else {
                continue;
            };
            units.extend(extractors.extract(&file.relative, &String::from_utf8_lossy(&bytes)));
            if units.len() >= self.functions {
                break;
            }
        }
        units.truncate(self.functions);
        let whole: Vec<String> = units.iter().map(|unit| unit.document_text()).collect();
        let outlines: Vec<String> = units.iter().map(|unit| unit.outline_text()).collect();
        let thread_counts = if self.threads.is_empty() {
            vec![EngineOptions::default().threads]
        } else {
            self.threads.clone()
        };
        let batches = if self.batches.is_empty() {
            vec![s1_engine::EmbedderSettings::BATCH]
        } else {
            self.batches.clone()
        };
        for retriever in &self.retrievers {
            let bundle = EmbedderBundle::open(self.models.bundle(retriever.bundle_name())?)?;
            for threads in &thread_counts {
                let mut embedder = Embedder::load(&bundle, *threads, Accelerator::Cpu)?;
                embedder.embed_documents(&whole[..whole.len().min(16)])?;
                for batch in &batches {
                    embedder.set_batch_size(*batch);
                    for (label, texts) in [("whole", &whole), ("outline", &outlines)] {
                        let started = Instant::now();
                        embedder.embed_documents(texts)?;
                        let seconds = started.elapsed().as_secs_f64();
                        println!(
                            "{:<13} threads {threads:>2} batch {batch:>3} {label:<8} {:>7.1} functions/s",
                            retriever.key(),
                            texts.len() as f64 / seconds
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
