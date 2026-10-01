use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, bail};
use clap::Args;
use serde::Deserialize;
use serde_json::json;

use s1grep::indexer::{Indexer, Pass};
use s1grep::models::{ModelDirectory, Retriever};
use s1grep::project::Project;
use s1grep::searcher::Searcher;
use s1grep::service::FileFilters;

/// One exam question, in the format the exam builders use: the answer is `path` (with the repository folder in
/// front) and `function`, plus any equally valid answers.
#[derive(Deserialize)]
struct ExamQuestion {
    text: String,
    language: String,
    path: String,
    function: String,
    #[serde(default)]
    also_accept: Vec<ExamAnswer>,
}

#[derive(Deserialize)]
struct ExamAnswer {
    path: String,
    function: String,
}

#[derive(Default)]
struct Tally {
    questions: usize,
    retriever_top1: usize,
    retriever_top5: usize,
    judge_top1: usize,
    fused_top1: usize,
    fused_top5: usize,
}

impl Tally {
    fn add(&mut self, retriever_rank: Option<usize>, judge_first: bool, fused_rank: Option<usize>) {
        self.questions += 1;
        self.retriever_top1 += usize::from(retriever_rank == Some(1));
        self.retriever_top5 += usize::from(retriever_rank.is_some_and(|rank| rank <= 5));
        self.judge_top1 += usize::from(judge_first);
        self.fused_top1 += usize::from(fused_rank == Some(1));
        self.fused_top5 += usize::from(fused_rank.is_some_and(|rank| rank <= 5));
    }

    fn to_json(&self) -> serde_json::Value {
        json!({"questions": self.questions, "retriever_top1": self.retriever_top1, "retriever_top5": self.retriever_top5,
               "judge_alone_top1": self.judge_top1, "fused_top1": self.fused_top1, "fused_top5": self.fused_top5})
    }
}

#[derive(Args)]
pub struct EvalCommand {
    /// Folder of exam files, one `<repository>.json` per repository
    #[arg(long)]
    exam: PathBuf,
    /// Folders that contain the repositories named by the exam files (repeatable)
    #[arg(long = "repos", required = true)]
    repository_folders: Vec<PathBuf>,
    #[arg(long, value_enum, default_value_t = Retriever::Granite)]
    retriever: Retriever,
    /// Search with the outline vectors only, as a project still being indexed is searched
    #[arg(long)]
    outline: bool,
    /// Candidates the judge reads (default: 5)
    #[arg(long)]
    judge_top: Option<usize>,
    #[arg(long)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl EvalCommand {
    pub fn run(self) -> anyhow::Result<()> {
        if std::env::var_os("XDG_CACHE_HOME").is_none() {
            bail!(
                "set XDG_CACHE_HOME to a lab folder: the exam indexes its repositories and must not write to the real cache"
            );
        }
        let started = Instant::now();
        let judged = self.judge_top.unwrap_or(self.retriever.default_judged());
        let mut searcher = Searcher::load(&self.models, self.retriever, true, self.threads)?;
        let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
        let mut search_seconds = Vec::new();
        let mut exam_files: Vec<PathBuf> = std::fs::read_dir(&self.exam)
            .with_context(|| format!("reading {}", self.exam.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .filter(|path| {
                !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('_'))
            })
            .collect();
        exam_files.sort();
        for exam_file in exam_files {
            let repository = exam_file.file_stem().unwrap().to_string_lossy().to_string();
            let root = self.repository_root(&repository)?;
            let questions: Vec<ExamQuestion> = serde_json::from_str(&std::fs::read_to_string(&exam_file)?)
                .with_context(|| format!("parsing {}", exam_file.display()))?;
            let project = Project {
                root: std::fs::canonicalize(&root)?,
                scope: None,
            };
            let mut store = project.open_store()?;
            let mut indexer = Indexer {
                store: &mut store,
                retriever: self.retriever,
            };
            indexer.scan(&project.root, &FileFilters::default().walk_options()?, &mut |_| {})?;
            let pass = if self.outline { Pass::Outline } else { Pass::Whole };
            indexer.embed(&mut searcher.embedder, pass, None, &mut |_| {})?;
            let key = pass.key(self.retriever);
            let units = store.searchable_units(key, key, None)?;
            for question in &questions {
                let answers: Vec<(String, String)> = std::iter::once((&question.path, &question.function))
                    .chain(
                        question
                            .also_accept
                            .iter()
                            .map(|answer| (&answer.path, &answer.function)),
                    )
                    .map(|(path, function)| {
                        (
                            path.strip_prefix(&format!("{repository}/")).unwrap_or(path).to_string(),
                            function.clone(),
                        )
                    })
                    .collect();
                let is_answer = |path: &str, name: &str| {
                    answers
                        .iter()
                        .any(|(expected_path, expected_name)| expected_path == path && expected_name == name)
                };
                let search_started = Instant::now();
                let hits = searcher.search(&question.text, &units, judged)?;
                search_seconds.push(search_started.elapsed().as_secs_f64());
                let fused_rank = hits
                    .iter()
                    .position(|hit| is_answer(&hit.unit.path, &hit.unit.name))
                    .map(|index| index + 1);
                let retriever_rank = hits
                    .iter()
                    .filter(|hit| is_answer(&hit.unit.path, &hit.unit.name))
                    .map(|hit| hit.retriever_rank)
                    .min();
                let judge_best = hits.iter().filter(|hit| hit.judge.is_some()).max_by(|left, right| {
                    left.judge
                        .unwrap()
                        .total_cmp(&right.judge.unwrap())
                        .then(right.retriever_rank.cmp(&left.retriever_rank))
                });
                let judge_first = judge_best.is_some_and(|hit| is_answer(&hit.unit.path, &hit.unit.name));
                for key in ["all".to_string(), question.language.clone()] {
                    tallies
                        .entry(key)
                        .or_default()
                        .add(retriever_rank, judge_first, fused_rank);
                }
            }
            eprintln!(
                "{repository}: {} questions over {} functions",
                questions.len(),
                units.len()
            );
        }
        search_seconds.sort_by(f64::total_cmp);
        let median = search_seconds
            .get(search_seconds.len() / 2)
            .copied()
            .unwrap_or_default();
        let summary = json!({
            "retriever": self.retriever.key(), "outline": self.outline, "judged": judged,
            "results": tallies.iter().map(|(key, tally)| (key.clone(), tally.to_json())).collect::<serde_json::Map<_, _>>(),
            "median_search_seconds": (median * 1000.0).round() / 1000.0,
            "total_seconds": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        });
        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }

    fn repository_root(&self, repository: &str) -> anyhow::Result<PathBuf> {
        for folder in &self.repository_folders {
            let candidate = folder.join(repository);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
        bail!("repository {repository} not found in any --repos folder")
    }
}
