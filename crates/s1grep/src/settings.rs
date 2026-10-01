//! Every tunable of s1grep in one place, grouped by what it controls. Rules about which files to read live in ignore
//! files instead (see `IndexSettings::DEFAULT_IGNORE`), so they can change without a new release.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;

/// How a search ranks and shows results.
pub struct SearchSettings;

impl SearchSettings {
    /// Results shown by default, and the most a caller may ask for.
    pub const TOP: usize = 5;
    pub const MAXIMUM_TOP: usize = 25;
    /// Functions the retriever brings for the judge to choose from.
    pub const CANDIDATES: usize = 25;
    /// Candidates the judge reads: 5 costs half of 10 on a CPU for 4 fewer right answers in 100 on the dev exam.
    pub const JUDGED: usize = 5;
    /// The question s1-code was trained with, followed by the search.
    pub const QUESTION_TEMPLATE: &'static str = "This code answers the search: ";
    /// Lines of code shown under each result in the terminal.
    pub const PREVIEW_LINES: usize = 6;
}

/// How projects are read and indexed.
pub struct IndexSettings;

impl IndexSettings {
    /// Default ignore rules, written to the user's config folder on first use so they can be edited.
    pub const DEFAULT_IGNORE: &'static str = include_str!("../assets/default.s1grepignore");
    /// Per-folder ignore file, read wherever it appears inside a project.
    pub const FOLDER_IGNORE_FILE: &'static str = ".s1grepignore";
    /// Source files read; the extractor understands Python only for now.
    pub const EXTENSIONS: [&'static str; 1] = ["py"];
    /// Larger files are generated code or data.
    pub const MAXIMUM_FILE_BYTES: u64 = 1_000_000;
    /// Functions embedded per step: small enough to report progress often.
    pub const EMBED_BATCH: usize = 32;
    /// Outlines embedded per step; they are short, so more fit in the same time.
    pub const OUTLINE_BATCH: usize = 128;
    /// A search waits for whole-source vectors only when this few functions miss one (seconds of work); a larger
    /// project gets outline vectors first, is searched with them, and gets the rest in the background.
    pub const INDEX_BEFORE_ANSWERING: usize = 64;
    /// Functions the background process embeds between two looks for new searches (a few seconds of work).
    pub const BACKGROUND_BATCH: usize = 8;
    /// Functions the background process fetches from the index at a time, so it does not query it for every batch.
    pub const BACKGROUND_QUEUE: usize = 512;
    pub const BACKGROUND_OUTLINE_QUEUE: usize = 2048;
    /// Functions to embed in the background before the time left is estimated; earlier guesses swing wildly.
    pub const ESTIMATE_AFTER: usize = 64;
    /// Files written to the index per transaction while reading a project.
    pub const SCAN_COMMIT_EVERY: usize = 500;
    /// Files read between two progress reports.
    pub const SCAN_REPORT_EVERY: usize = 500;
}

/// The background server that keeps the models in memory between searches. Searches start it on their own.
pub struct ServerSettings;

impl ServerSettings {
    /// It stops after this long without searches, releasing the memory.
    pub const IDLE: Duration = Duration::from_secs(30 * 60);
    /// How long a search waits for a freshly started server to load the models.
    pub const START_TIMEOUT: Duration = Duration::from_secs(180);
    /// A process that does not answer a ping within this time is busy (or hung) and is not used for searches.
    pub const PING_TIMEOUT: Duration = Duration::from_secs(4);
    /// After asking it to stop, how long to wait before terminating it, and how long a terminated one may take.
    pub const STOP_TIMEOUT: Duration = Duration::from_secs(5);
    pub const KILL_TIMEOUT: Duration = Duration::from_secs(5);
    pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
    /// Time a client has to send its request once connected.
    pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
    pub const POLL_INTERVAL: Duration = Duration::from_millis(100);
    pub const INFO_FILE: &'static str = "server.json";
    pub const LOCK_FILE: &'static str = "server.lock";
    pub const LOG_FILE: &'static str = "server.log";
}

/// Terminal output.
pub struct DisplaySettings;

impl DisplaySettings {
    /// indicatif layouts: a spinner for waits of unknown length, bars for reading files, indexing and downloading.
    pub const SPINNER_TEMPLATE: &'static str = "{spinner:.cyan} {msg} {elapsed:.dim}";
    pub const READING_TEMPLATE: &'static str = "{spinner:.cyan} Reading files {pos}/{len}";
    pub const OUTLINING_TEMPLATE: &'static str =
        "{spinner:.cyan} Mapping  {bar:28.cyan/dim} {human_pos}/{human_len} functions {eta:.dim}";
    pub const INDEXING_TEMPLATE: &'static str =
        "{spinner:.cyan} Indexing {bar:28.cyan/dim} {human_pos}/{human_len} functions {msg:.dim}";
    pub const DOWNLOAD_TEMPLATE: &'static str =
        "{spinner:.cyan} {msg} {bar:28.cyan/dim} {bytes}/{total_bytes} {bytes_per_sec:.dim} {eta:.dim}";
    /// Filled, current and empty cells of every bar.
    pub const BAR_CHARACTERS: &'static str = "━╸━";
    pub const STATUS_BAR_WIDTH: usize = 24;
    /// How often a plain progress line is printed when output is redirected.
    pub const PLAIN_INTERVAL: Duration = Duration::from_secs(10);
    /// Frame time of the animation shown while the models load.
    pub const SPINNER_INTERVAL: Duration = Duration::from_millis(80);
}

/// One file of a published model, pinned by its size and SHA-256.
pub struct ModelFile {
    /// Path inside the Hugging Face repository.
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// A model bundle as published on Hugging Face, pinned to one commit so that every install gets the same bytes.
pub struct ModelRelease {
    /// Folder name inside the model directory.
    pub bundle: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub files: &'static [ModelFile],
}

/// Model bundles and where they come from.
pub struct ModelSettings;

impl ModelSettings {
    pub const JUDGE_BUNDLE: &'static str = "s1-code-v3-onnx";
    pub const RETRIEVER_BUNDLE: &'static str = "granite-278m-onnx";
    pub const HUB: &'static str = "https://huggingface.co";
    /// Downloads larger than this print their progress.
    pub const DOWNLOAD_PROGRESS_FROM: u64 = 50_000_000;
    /// Attempts per file when Hugging Face is busy (429) or fails for a moment (5xx, network).
    pub const DOWNLOAD_ATTEMPTS: u32 = 6;
    /// Waits before the next attempt: after a 429 without Retry-After, after a 5xx, after a network error, and the
    /// most a Retry-After is honoured.
    pub const WAIT_WHEN_BUSY: Duration = Duration::from_secs(20);
    pub const WAIT_AFTER_SERVER_ERROR: Duration = Duration::from_secs(10);
    pub const WAIT_AFTER_NETWORK_ERROR: Duration = Duration::from_secs(5);
    pub const DOWNLOAD_MAXIMUM_WAIT: Duration = Duration::from_secs(60);

    pub const RELEASES: [ModelRelease; 2] = [
        ModelRelease {
            bundle: Self::JUDGE_BUNDLE,
            repository: "api-service-sac/s1-code-v3",
            revision: "9e5cdd6820c068d92a7a446191f1c1647692267a",
            files: &[
                ModelFile {
                    path: "onnx/decision_config.json",
                    size: 512,
                    sha256: "8e5a5eab0e290b95ff77bfa921d644e3f763fee2fd95dc021f191c9388882f94",
                },
                ModelFile {
                    path: "onnx/model.onnx",
                    size: 1_290_355_173,
                    sha256: "856a3026b23fc9cba051669702f6492b487a831ee58dbd61f0465bb7f2964ceb",
                },
                ModelFile {
                    path: "onnx/tokenizer.json",
                    size: 34_363_188,
                    sha256: "609d8f4c067cd3950f88594c5a802616cea245823836ef5848ee4fc40aab5b6f",
                },
                ModelFile {
                    path: "onnx/tokenizer_config.json",
                    size: 624,
                    sha256: "f2ff584a8f78ac9f3b6fd0afcf9ab310e41c2d445ef628f9ab4b0d594e992b7a",
                },
            ],
        },
        ModelRelease {
            bundle: Self::RETRIEVER_BUNDLE,
            repository: "api-service-sac/granite-embedding-278m-multilingual-onnx",
            revision: "b795cbc00b23bcaafbbbba6b242448104cc62ec0",
            files: &[
                ModelFile {
                    path: "embedder_config.json",
                    size: 218,
                    sha256: "82c5f97e83ecd0c39faa68019a71ca10580528a5b14161059351587136359f2a",
                },
                ModelFile {
                    path: "model.onnx",
                    size: 1_111_543_374,
                    sha256: "ce7cf0e3f9ba39989c0956a8b44987b60f75dc69e6f6ed1a71f3b18188973d89",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 9_081_351,
                    sha256: "2a0d7366dd7780ea36cc42431dd74cd79289b783ab01acd33013fcc96865a8e9",
                },
            ],
        },
    ];
}

/// Coding agents: the Claude Code skill and the MCP server.
pub struct AgentSettings;

impl AgentSettings {
    pub const SKILL: &'static str = include_str!("../assets/SKILL.md");
}

/// Platform details. Only the Windows build reads them.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct PlatformSettings;

#[cfg_attr(not(windows), allow(dead_code))]
impl PlatformSettings {
    /// ONNX Runtime on Windows, loaded from next to s1grep.exe (Linux links it statically).
    pub const WINDOWS_RUNTIME_LIBRARY: &'static str = "onnxruntime.dll";
    /// Windows process creation flags for the background process: no console, and Ctrl+C elsewhere does not reach it.
    pub const WINDOWS_DETACHED_PROCESS: u32 = 0x0000_0008;
    pub const WINDOWS_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
}

/// The development commands `bench` and `decide`.
pub struct BenchSettings;

impl BenchSettings {
    /// Bundle they load when no `--model` is given.
    pub const MODEL: &'static str = "laya-multilingual";
    /// The batched measurement: this many fragments of this many tokens.
    pub const BATCH_SIZE: usize = 8;
    pub const BATCH_TOKENS: usize = 128;
}

/// The MCP server for coding agents.
pub struct McpSettings;

impl McpSettings {
    pub const PROTOCOL_VERSIONS: [&'static str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
    pub const TOOL: &'static str = "search_code";
    pub const PREVIEW_LINES: usize = 40;
    pub const INSTRUCTIONS: &'static str = "s1grep finds functions by what they do, from a description in English or \
        Spanish, and returns their file, lines and code. Use it when you know the behaviour but not where it lives; \
        use grep for exact names or strings. It reads Python repositories.";
}

/// The user's configuration folder: `$XDG_CONFIG_HOME/s1grep`, `~/.config/s1grep`, or `%APPDATA%\s1grep`.
pub struct ConfigDirectory;

impl ConfigDirectory {
    const IGNORE_FILE: &'static str = "ignore";

    pub fn root() -> anyhow::Result<PathBuf> {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(config).join("s1grep"));
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(roaming).join("s1grep"));
        }
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home).join(".config").join("s1grep"))
    }

    /// The global ignore file, created with the default rules the first time it is needed.
    pub fn ignore_file() -> anyhow::Result<PathBuf> {
        let path = Self::root()?.join(Self::IGNORE_FILE);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(&path, IndexSettings::DEFAULT_IGNORE)
                .with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(path)
    }
}
