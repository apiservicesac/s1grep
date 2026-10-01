use std::io::IsTerminal;
use std::time::Instant;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use crate::settings::DisplaySettings;

/// What s1grep is doing, sent to whoever waits for it: the terminal, or a search answered by the background process.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum IndexEvent {
    /// Loading the models into this process (about 2.9 GB), before anything else can happen.
    LoadingModels,
    /// Starting the background process that keeps the models loaded between searches.
    StartingServer,
    /// Reading files and splitting them into functions.
    Scanning { done: usize, files: usize },
    Scanned {
        files: usize,
        changed: usize,
        #[serde(default)]
        skipped: usize,
        functions: usize,
        seconds: f64,
    },
    /// Computing the quick outline vectors, so that the whole project can be searched.
    Outlining { done: usize, total: usize, seconds: f64 },
    /// Computing vectors for the functions that have none yet.
    Embedding { done: usize, total: usize, seconds: f64 },
    /// Another process holds the index; this search uses what is already there.
    Busy { pid: Option<u32> },
}

/// Formats numbers and durations the way the summaries show them.
pub struct Units;

impl Units {
    pub fn count(value: usize) -> String {
        let digits = value.to_string();
        let mut grouped = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
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

/// Which animation is on screen, so that consecutive events update it instead of replacing it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Animation {
    Spinner,
    Reading,
    Outlining,
    Indexing,
}

/// The animations on stderr, drawn by indicatif: a spinner while the models load, bars while files are read and
/// functions indexed. When stderr is not a terminal nothing is animated and a plain line is printed now and then.
pub struct ProgressDisplay {
    terminal: bool,
    current: Option<(Animation, ProgressBar)>,
    last_plain_line: Option<Instant>,
}

impl ProgressDisplay {
    pub fn new() -> Self {
        Self {
            terminal: std::io::stderr().is_terminal(),
            current: None,
            last_plain_line: None,
        }
    }

    pub fn show(&mut self, event: &IndexEvent) {
        match event {
            IndexEvent::LoadingModels => self.spinner("Loading the models · about 10 s"),
            IndexEvent::StartingServer => self.spinner("Loading the models · only once, about 10 s"),
            IndexEvent::Scanning { done, files } => {
                self.bar(Animation::Reading, *files, *done, "");
                self.plain(&format!("Reading files {done}/{files}"), false);
            }
            IndexEvent::Scanned {
                files,
                changed,
                skipped,
                functions,
                seconds,
            } => {
                self.clear();
                if *skipped > 0 {
                    self.line(&self.warn(&format!(
                        "Skipped {} files that changed or became unreadable while being read",
                        Units::count(*skipped)
                    )));
                }
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
                let rate = if *seconds > DisplaySettings::RATE_AFTER_SECONDS {
                    *done as f64 / seconds
                } else {
                    0.0
                };
                let left = if rate > 0.0 && done < total {
                    format!(
                        "· {rate:.1}/s · ~{} left",
                        Units::duration((total - done) as f64 / rate)
                    )
                } else {
                    String::new()
                };
                self.bar(Animation::Indexing, *total, *done, &left);
                self.plain(
                    &format!(
                        "Indexing {}/{} functions {left}",
                        Units::count(*done),
                        Units::count(*total)
                    ),
                    done == total,
                );
            }
            IndexEvent::Outlining { done, total, .. } => {
                self.bar(Animation::Outlining, *total, *done, "");
                self.plain(
                    &format!("Mapping {}/{} functions", Units::count(*done), Units::count(*total)),
                    done == total,
                );
            }
            IndexEvent::Busy { pid } => {
                let holder = pid.map(|pid| format!(" (process {pid})")).unwrap_or_default();
                self.line(&self.warn(&format!(
                    "Another s1grep is indexing this project{holder}; searching what is already indexed."
                )));
            }
        }
    }

    /// Removes the animation so that results print cleanly.
    pub fn clear(&mut self) {
        if let Some((_, bar)) = self.current.take() {
            bar.finish_and_clear();
        }
    }

    pub fn line(&mut self, text: &str) {
        self.clear();
        eprintln!("{text}");
    }

    pub fn dim(&self, text: &str) -> String {
        style(text).for_stderr().dim().to_string()
    }

    pub fn warn(&self, text: &str) -> String {
        style(text).for_stderr().yellow().to_string()
    }

    pub fn good(&self, text: &str) -> String {
        style(text).for_stderr().green().to_string()
    }

    pub fn accent(&self, text: &str) -> String {
        style(text).for_stderr().cyan().to_string()
    }

    /// A spinner for waits of unknown length; it keeps turning on its own while the caller is blocked.
    fn spinner(&mut self, message: &str) {
        self.clear();
        if !self.terminal {
            eprintln!("{message}…");
            return;
        }
        let spinner = ProgressBar::new_spinner().with_message(message.to_string());
        spinner.set_style(ProgressStyle::with_template(DisplaySettings::SPINNER_TEMPLATE).expect("valid template"));
        spinner.enable_steady_tick(DisplaySettings::SPINNER_INTERVAL);
        self.current = Some((Animation::Spinner, spinner));
    }

    /// Moves the bar of this kind, creating it in place of whatever was shown.
    fn bar(&mut self, animation: Animation, length: usize, position: usize, message: &str) {
        if !self.terminal {
            return;
        }
        if !matches!(&self.current, Some((current, _)) if *current == animation) {
            self.clear();
            let template = match animation {
                Animation::Reading => DisplaySettings::READING_TEMPLATE,
                Animation::Outlining => DisplaySettings::OUTLINING_TEMPLATE,
                Animation::Indexing | Animation::Spinner => DisplaySettings::INDEXING_TEMPLATE,
            };
            let bar = ProgressBar::new(length as u64);
            bar.set_style(
                ProgressStyle::with_template(template)
                    .expect("valid template")
                    .progress_chars(DisplaySettings::BAR_CHARACTERS),
            );
            bar.enable_steady_tick(DisplaySettings::SPINNER_INTERVAL);
            self.current = Some((animation, bar));
        }
        if let Some((_, bar)) = &self.current {
            bar.set_length(length as u64);
            bar.set_position(position as u64);
            bar.set_message(message.to_string());
        }
    }

    /// A line every few seconds when nothing can be animated (output redirected to a file or another program).
    fn plain(&mut self, text: &str, finished: bool) {
        if self.terminal {
            return;
        }
        let now = Instant::now();
        if finished
            || self
                .last_plain_line
                .is_none_or(|last| now - last >= DisplaySettings::PLAIN_INTERVAL)
        {
            eprintln!("{text}");
            self.last_plain_line = Some(now);
        }
    }
}

impl Default for ProgressDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProgressDisplay {
    fn drop(&mut self) {
        self.clear();
    }
}
