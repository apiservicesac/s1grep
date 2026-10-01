use std::io::{IsTerminal, Write};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::settings::DisplaySettings;

/// What the indexer is doing, sent to whoever waits for it: the terminal, or a client of `s1grep serve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum IndexEvent {
    /// Loading the models into this process (about 2.4 GB), before anything else can happen.
    LoadingModels,
    /// Reading files and splitting them into functions.
    Scanning { done: usize, files: usize },
    Scanned {
        files: usize,
        changed: usize,
        functions: usize,
        seconds: f64,
    },
    /// Computing vectors for the functions that have none yet.
    Embedding { done: usize, total: usize, seconds: f64 },
    /// Another process holds the index; this search uses what is already there.
    Busy { pid: Option<u32> },
}

/// Formats numbers and durations the way the progress line shows them.
pub struct Units;

impl Units {
    pub fn count(value: usize) -> String {
        let digits = value.to_string();
        let mut grouped = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        grouped
    }

    pub fn duration(seconds: f64) -> String {
        match seconds {
            value if value < 60.0 => format!("{:.0} s", value.max(1.0)),
            value if value < 3600.0 => format!("{:.0} min", (value / 60.0).ceil()),
            value => format!("{:.1} h", value / 3600.0),
        }
    }
}

/// Draws index events on stderr: a live bar on a terminal, a line every few seconds otherwise.
pub struct ProgressDisplay {
    terminal: bool,
    color: bool,
    last_line: Option<Instant>,
    drawn: bool,
}

impl ProgressDisplay {
    pub fn new() -> Self {
        let terminal = std::io::stderr().is_terminal();
        Self {
            terminal,
            color: terminal && std::env::var_os("NO_COLOR").is_none(),
            last_line: None,
            drawn: false,
        }
    }

    pub fn show(&mut self, event: &IndexEvent) {
        match event {
            IndexEvent::LoadingModels => {
                self.live(
                    "Loading the models (about 10 s; `s1grep serve` keeps them loaded between searches)…",
                    true,
                );
            }
            IndexEvent::Scanning { done, files } => {
                self.live(
                    &format!("Reading files {} / {}", Units::count(*done), Units::count(*files)),
                    false,
                );
            }
            IndexEvent::Scanned {
                files,
                changed,
                functions,
                seconds,
            } => {
                self.clear();
                if *changed > 0 {
                    self.line(&self.dim(&format!(
                        "Read {} files ({} new or changed) · {} functions · {:.1} s",
                        Units::count(*files),
                        Units::count(*changed),
                        Units::count(*functions),
                        seconds
                    )));
                }
            }
            IndexEvent::Embedding { done, total, seconds } => {
                let rate = if *seconds > 0.5 { *done as f64 / seconds } else { 0.0 };
                let remaining = if rate > 0.0 {
                    format!(" · ~{} left", Units::duration((total - done) as f64 / rate))
                } else {
                    String::new()
                };
                let speed = if rate > 0.0 {
                    format!(" · {rate:.0}/s")
                } else {
                    String::new()
                };
                let text = format!(
                    "Indexing {} {} / {} functions{speed}{remaining}",
                    self.bar(*done, *total),
                    Units::count(*done),
                    Units::count(*total)
                );
                self.live(&text, done == total);
            }
            IndexEvent::Busy { pid } => {
                self.clear();
                let holder = pid.map(|pid| format!(" (process {pid})")).unwrap_or_default();
                self.line(&self.warn(&format!(
                    "Another s1grep is indexing this project{holder}; searching what is already indexed."
                )));
            }
        }
    }

    /// Removes the live line so that results print cleanly.
    pub fn clear(&mut self) {
        if self.terminal && self.drawn {
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
            self.drawn = false;
        }
    }

    pub fn line(&mut self, text: &str) {
        self.clear();
        eprintln!("{text}");
    }

    pub fn dim(&self, text: &str) -> String {
        self.paint(text, "2")
    }

    pub fn warn(&self, text: &str) -> String {
        self.paint(text, "33")
    }

    pub fn good(&self, text: &str) -> String {
        self.paint(text, "32")
    }

    pub fn accent(&self, text: &str) -> String {
        self.paint(text, "36")
    }

    fn paint(&self, text: &str, style: &str) -> String {
        if self.color {
            format!("\x1b[{style}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn bar(&self, done: usize, total: usize) -> String {
        let filled = if total == 0 {
            DisplaySettings::PROGRESS_BAR_WIDTH
        } else {
            done * DisplaySettings::PROGRESS_BAR_WIDTH / total
        };
        let bar = format!(
            "{}{}",
            "━".repeat(filled),
            " ".repeat(DisplaySettings::PROGRESS_BAR_WIDTH - filled)
        );
        if self.color {
            format!(
                "\x1b[36m{}\x1b[0m\x1b[2m{}\x1b[0m",
                "━".repeat(filled),
                "━".repeat(DisplaySettings::PROGRESS_BAR_WIDTH - filled)
            )
        } else {
            format!("[{bar}]")
        }
    }

    fn live(&mut self, text: &str, finished: bool) {
        let now = Instant::now();
        if self.terminal {
            if finished
                || self
                    .last_line
                    .is_none_or(|last| now - last >= DisplaySettings::LIVE_INTERVAL)
            {
                eprint!("\r\x1b[2K{text}");
                let _ = std::io::stderr().flush();
                self.drawn = true;
                self.last_line = Some(now);
            }
        } else if finished
            || self
                .last_line
                .is_none_or(|last| now - last >= DisplaySettings::PLAIN_INTERVAL)
        {
            eprintln!("{text}");
            self.last_line = Some(now);
        }
    }
}
