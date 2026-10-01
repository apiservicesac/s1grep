use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, bail};
use clap::Args;
use s1_index::{LexicalIndex, VectorIndex};
use serde::Deserialize;
use serde_json::json;

use s1grep::indexer::{Indexer, Pass};
use s1grep::models::{ModelDirectory, Retriever};
use s1grep::project::Project;
use s1grep::searcher::{SearchIndexes, Searcher};
use s1grep::service::FileFilters;
use s1grep::settings::SearchSettings;

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

/// What each exam question is turned into before it is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum QueryKind {
    /// The question as written, in English or Spanish
    Natural,
    /// The answer's function name, as someone looking for a known name would type it
    Identifier,
    /// The longest quoted text of three or more words inside the answer, like a pasted error message
    Literal,
}

impl QueryKind {
    /// The query for `question` whose answer is `answer`; `None` when this kind has nothing to offer.
    fn query(self, question: &str, answer: Option<&s1_index::CodeUnit>) -> Option<String> {
        match self {
            Self::Natural => Some(question.to_string()),
            Self::Identifier => answer
                .and_then(|unit| unit.name.rsplit('.').next())
                .map(ToString::to_string),
            Self::Literal => answer.and_then(|unit| Self::longest_literal(&unit.source)),
        }
    }

    fn longest_literal(source: &str) -> Option<String> {
        let mut best: Option<String> = None;
        for quote in ['"', '\''] {
            for (position, text) in source.split(quote).enumerate() {
                let inside = position % 2 == 1 && !text.contains('\n');
                if inside
                    && text.split_whitespace().count() >= 3
                    && best.as_ref().is_none_or(|current| text.len() > current.len())
                {
                    best = Some(text.trim().to_string());
                }
            }
        }
        best
    }
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
    /// Stop judging a search's candidates once one scores at least this probability (default: the shipped setting)
    #[arg(long, conflicts_with = "judge_all")]
    early_stop: Option<f64>,
    /// Judge every candidate, with no early stop
    #[arg(long)]
    judge_all: bool,
    /// Write one JSON line per question (query, language, where the answer ranked, what found the first result)
    #[arg(long)]
    details: Option<PathBuf>,
    /// What to search for each question: the question itself, the answer's name, or a quoted text from it
    #[arg(long, value_enum, default_value_t = QueryKind::Natural)]
    queries: QueryKind,
    /// Weight of the word index's list in the merge (default: the shipped setting; 0 leaves it out)
    #[arg(long, default_value_t = SearchSettings::LEXICAL_WEIGHT)]
    lexical_weight: f64,
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
        let early_stop = if self.judge_all {
            None
        } else {
            Some(self.early_stop.unwrap_or(SearchSettings::JUDGE_EARLY_STOP))
        };
        searcher.set_early_stop(early_stop);
        searcher.set_lexical_weight(self.lexical_weight);
        let mut judged_total = 0;
        let mut details = Vec::new();
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
            let project = Project::at_root(&root)?;
            let mut store = project.open_store()?;
            let mut indexer = Indexer {
                store: &mut store,
                retriever: self.retriever,
            };
            indexer.scan(&project.root, &FileFilters::default().walk_options()?, &mut |_| {})?;
            let pass = if self.outline { Pass::Outline } else { Pass::Whole };
            indexer.embed(searcher.embedder_for(pass), pass, None, &mut |_| {})?;
            let key = pass.key(self.retriever);
            let lexical = if self.lexical_weight > 0.0 {
                let mut words = LexicalIndex::open(&project.folder.lexical())?;
                words.rebuild(&store.all_units()?)?;
                Some(words)
            } else {
                None
            };
            let all_units = store.all_units()?;
            let rows = VectorIndex::new(store.vector_rows(&key, None)?);
            let index = match pass {
                Pass::Whole => SearchIndexes {
                    whole: rows,
                    outline: VectorIndex::new(Vec::new()),
                },
                Pass::Outline => SearchIndexes {
                    whole: VectorIndex::new(Vec::new()),
                    outline: rows,
                },
            };
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
                let answer = all_units
                    .iter()
                    .find(|stored| is_answer(&stored.unit.path, &stored.unit.name))
                    .map(|stored| &stored.unit);
                let Some(query) = self.queries.query(&question.text, answer) else {
                    continue;
                };
                let search_started = Instant::now();
                let hits = searcher.search(&query, &index, lexical.as_ref(), |_| true, &store, judged)?;
                search_seconds.push(search_started.elapsed().as_secs_f64());
                judged_total += hits.iter().filter(|hit| hit.judge.is_some()).count();
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
                details.push(
                    json!({"repository": repository, "query": query, "language": question.language,
                    "fused_rank": fused_rank, "first_found_by": hits.first().map(|hit| format!("{:?}", hit.found_by))}),
                );
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
                index.rows().count()
            );
        }
        if let Some(path) = &self.details {
            let lines: Vec<String> = details.iter().map(ToString::to_string).collect();
            std::fs::write(path, lines.join("\n") + "\n").with_context(|| format!("writing {}", path.display()))?;
        }
        search_seconds.sort_by(f64::total_cmp);
        let median = search_seconds
            .get(search_seconds.len() / 2)
            .copied()
            .unwrap_or_default();
        let summary = json!({
            "retriever": self.retriever.key(), "outline": self.outline, "judged": judged, "early_stop": early_stop, "lexical_weight": self.lexical_weight, "queries": format!("{:?}", self.queries),
            "mean_judged": judged_total as f64 / search_seconds.len().max(1) as f64,
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
