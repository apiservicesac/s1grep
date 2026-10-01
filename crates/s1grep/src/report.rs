use std::fmt::Write;
use std::path::Path;

use crate::service::SearchResponse;

/// Human-readable results: one heading per function with its location and score, then the first lines of its code.
pub struct TextReport<'a> {
    pub response: &'a SearchResponse,
    pub preview_lines: usize,
    pub color: bool,
}

impl TextReport<'_> {
    pub fn render(&self) -> String {
        let mut text = String::new();
        if self.response.results.is_empty() {
            let _ = writeln!(text, "No functions found in {}.", self.response.root.display());
            return text;
        }
        for result in &self.response.results {
            let location = format!(
                "{}:{}-{}",
                self.display_path(&result.path),
                result.start_line,
                result.end_line
            );
            let score = match result.judge {
                Some(probability) => format!("judge {:.0}%", probability * 100.0),
                None => format!("{:.2} similar", result.similarity),
            };
            let _ = writeln!(
                text,
                "{} {}  {}  {}",
                self.paint(&format!("{:>2}.", result.rank), "2"),
                self.paint(&location, "1;36"),
                self.paint(&result.name, "1"),
                self.paint(&score, "2")
            );
            let lines = Self::dedented(&result.source);
            for line in lines.iter().take(self.preview_lines) {
                let _ = writeln!(text, "    {line}");
            }
            if lines.len() > self.preview_lines {
                let _ = writeln!(
                    text,
                    "    {}",
                    self.paint(&format!("… {} more lines", lines.len() - self.preview_lines), "2")
                );
            }
            text.push('\n');
        }
        text
    }

    /// The source starts at `def` but keeps the indentation of its body, so a method's body sits one level too deep.
    /// Moves the body back so that it is indented exactly one level under the first line.
    fn dedented(source: &str) -> Vec<String> {
        let mut lines = source.lines();
        let first = lines.next().unwrap_or_default().to_string();
        let body: Vec<&str> = lines.collect();
        let indentation = |line: &&str| line.len() - line.trim_start().len();
        let shallowest = body
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(indentation)
            .min()
            .unwrap_or(0);
        let surplus = shallowest.saturating_sub(4);
        std::iter::once(first)
            .chain(body.iter().map(|line| {
                if indentation(line) >= surplus {
                    line[surplus..].to_string()
                } else {
                    line.to_string()
                }
            }))
            .collect()
    }

    /// The result's path as the user would type it from the current folder.
    fn display_path(&self, relative: &str) -> String {
        let absolute = self.response.root.join(relative);
        let current = std::env::current_dir()
            .ok()
            .and_then(|folder| std::fs::canonicalize(folder).ok());
        match current.as_deref().and_then(|folder| absolute.strip_prefix(folder).ok()) {
            Some(path) if path != Path::new("") => path.display().to_string(),
            _ => absolute.display().to_string(),
        }
    }

    fn paint(&self, text: &str, style: &str) -> String {
        if self.color {
            format!("\x1b[{style}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}
