use std::path::PathBuf;

use clap::Args;
use s1_engine::{Accelerator, Embedder, EmbedderBundle, EngineOptions};
use s1_index::{LexicalIndex, VectorIndex};
use s1grep::indexer::Pass;
use s1grep::models::{ModelDirectory, Retriever};
use s1grep::project::Project;

#[derive(Args)]
pub struct ExplainCommand {
    /// Indexed project
    path: PathBuf,
    /// The search
    query: String,
    /// Text a function name must contain to be shown with its rank in each list
    #[arg(long)]
    expect: Vec<String>,
    /// How far down each list to look
    #[arg(long, default_value_t = 200)]
    depth: usize,
    #[command(flatten)]
    models: ModelDirectory,
}

impl ExplainCommand {
    /// Where the expected functions rank in each list a search draws from, on an existing index: whole-source
    /// vectors, outline vectors, and the word index by every word.
    pub fn run(self) -> anyhow::Result<()> {
        let project = Project::locate(&self.path)?;
        let store = project.open_store()?;
        let retriever = Retriever::Granite;
        let lexical = LexicalIndex::open(&project.folder.lexical())?;
        for (label, pass, partner) in [
            ("whole", Pass::Whole, retriever),
            ("outline", Pass::Outline, retriever.outline_partner()),
        ] {
            let index = VectorIndex::new(store.vector_rows(&pass.key(retriever), None)?);
            if index.is_empty() {
                continue;
            }
            let bundle = EmbedderBundle::open(self.models.bundle(partner.bundle_name())?)?;
            let mut embedder = Embedder::load(&bundle, EngineOptions::default().threads, Accelerator::Cpu)?;
            let query = embedder.embed_query(&self.query)?;
            let nearest = index.nearest(&query, self.depth, |_| true);
            let ids: Vec<i64> = nearest
                .iter()
                .map(|(position, _)| index.row(*position).unit_id)
                .collect();
            self.report(
                label,
                &store
                    .units_by_ids(&ids)?
                    .iter()
                    .map(|unit| unit.unit.name.clone())
                    .collect::<Vec<_>>(),
            );
        }
        for (label, hits) in [
            ("words", lexical.search(&self.query, self.depth, |_| true)?),
            ("phrase", lexical.search_phrase(&self.query, self.depth, |_| true)?),
        ] {
            let ids: Vec<i64> = hits.iter().map(|hit| hit.unit_id).collect();
            self.report(
                label,
                &store
                    .units_by_ids(&ids)?
                    .iter()
                    .map(|unit| unit.unit.name.clone())
                    .collect::<Vec<_>>(),
            );
        }
        Ok(())
    }

    fn report(&self, label: &str, names: &[String]) {
        println!("{label}: top 3 {:?}", &names[..names.len().min(3)]);
        for (rank, name) in names.iter().enumerate() {
            if self.expect.iter().any(|expected| name.contains(expected.as_str())) {
                println!("  #{:<4} {name}", rank + 1);
            }
        }
    }
}
