use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, bail};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

use crate::settings::{DisplaySettings, ModelFile, ModelRelease, ModelSettings};

/// Why one download attempt failed: worth retrying or not.
enum Failure {
    /// Hugging Face is busy or failed for a moment; wait this long and try again.
    Retry(Duration, String),
    Fatal(anyhow::Error),
}

/// Downloads the pinned model files from Hugging Face. No API calls: every file and its SHA-256 are known in advance.
/// `HF_TOKEN` is sent when set, for repositories that are still private.
pub struct ModelInstaller {
    agent: ureq::Agent,
    token: Option<String>,
}

impl Default for ModelInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelInstaller {
    pub fn new() -> Self {
        Self {
            // Status codes are read, not turned into errors, so a 429 keeps its Retry-After header.
            agent: ureq::Agent::config_builder().http_status_as_error(false).build().into(),
            token: std::env::var("HF_TOKEN").ok().filter(|token| !token.is_empty()),
        }
    }

    /// Whether every file of every bundle is in `root` with its expected size.
    pub fn is_complete(root: &Path) -> bool {
        ModelSettings::RELEASES.iter().all(|release| {
            release.files.iter().all(|file| {
                Self::target(root, release, file)
                    .metadata()
                    .is_ok_and(|meta| meta.len() == file.size)
            })
        })
    }

    pub fn install(&self, root: &Path, force: bool) -> anyhow::Result<()> {
        for release in &ModelSettings::RELEASES {
            std::fs::create_dir_all(root.join(release.bundle))
                .with_context(|| format!("creating {}", root.join(release.bundle).display()))?;
            for file in release.files {
                let target = Self::target(root, release, file);
                let complete = target.metadata().is_ok_and(|metadata| metadata.len() == file.size);
                if complete && !force {
                    continue;
                }
                eprintln!(
                    "{} · {} ({:.1} MB)",
                    release.bundle,
                    Self::name(file),
                    file.size as f64 / 1e6
                );
                self.download_with_retries(release, file, &target)?;
            }
        }
        Ok(())
    }

    fn target(root: &Path, release: &ModelRelease, file: &ModelFile) -> std::path::PathBuf {
        root.join(release.bundle).join(Self::name(file))
    }

    fn name(file: &ModelFile) -> &str {
        file.path.rsplit('/').next().unwrap_or(file.path)
    }

    fn download_with_retries(&self, release: &ModelRelease, file: &ModelFile, target: &Path) -> anyhow::Result<()> {
        for attempt in 1..=ModelSettings::DOWNLOAD_ATTEMPTS {
            match self.download(release, file, target) {
                Ok(()) => return Ok(()),
                Err(Failure::Fatal(error)) => return Err(error),
                Err(Failure::Retry(wait, reason)) if attempt < ModelSettings::DOWNLOAD_ATTEMPTS => {
                    eprintln!(
                        "  {reason}; trying again in {} s ({attempt}/{})",
                        wait.as_secs(),
                        ModelSettings::DOWNLOAD_ATTEMPTS
                    );
                    std::thread::sleep(wait);
                }
                Err(Failure::Retry(_, reason)) => bail!(
                    "{reason}; gave up after {} attempts, try `s1grep setup` again later",
                    ModelSettings::DOWNLOAD_ATTEMPTS
                ),
            }
        }
        unreachable!("the last attempt either succeeds or gives up")
    }

    /// One attempt: streams the file next to its target, checks size and SHA-256, then moves it into place.
    fn download(&self, release: &ModelRelease, file: &ModelFile, target: &Path) -> Result<(), Failure> {
        let url = format!(
            "{}/{}/resolve/{}/{}",
            ModelSettings::HUB,
            release.repository,
            release.revision,
            file.path
        );
        let mut request = self.agent.get(&url);
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(error) => {
                return Err(Failure::Retry(
                    ModelSettings::WAIT_AFTER_NETWORK_ERROR,
                    format!("network error: {error}"),
                ));
            }
        };
        match response.status().as_u16() {
            200..=299 => {}
            status @ (401 | 403 | 404) => {
                return Err(Failure::Fatal(anyhow::anyhow!(
                    "{} answered {status}: the model is not public yet (set HF_TOKEN if you have access)",
                    release.repository
                )));
            }
            429 => {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .map_or(ModelSettings::WAIT_WHEN_BUSY, Duration::from_secs)
                    .min(ModelSettings::DOWNLOAD_MAXIMUM_WAIT);
                return Err(Failure::Retry(
                    wait,
                    "Hugging Face asked to slow down (429)".to_string(),
                ));
            }
            status if status >= 500 => {
                return Err(Failure::Retry(
                    ModelSettings::WAIT_AFTER_SERVER_ERROR,
                    format!("Hugging Face failed for a moment ({status})"),
                ));
            }
            status => return Err(Failure::Fatal(anyhow::anyhow!("{url} answered {status}"))),
        }
        let partial = target.with_extension("part");
        let result = Self::stream(response, file, &partial);
        match result {
            Ok(()) => std::fs::rename(&partial, target).map_err(|error| Failure::Fatal(error.into())),
            Err(failure) => {
                let _ = std::fs::remove_file(&partial);
                Err(failure)
            }
        }
    }

    fn stream(response: ureq::http::Response<ureq::Body>, file: &ModelFile, partial: &Path) -> Result<(), Failure> {
        let fatal = |error: std::io::Error| {
            Failure::Fatal(anyhow::Error::from(error).context(format!("writing {}", partial.display())))
        };
        let mut reader = response.into_body().into_reader();
        let mut output = std::fs::File::create(partial).map_err(fatal)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; ModelSettings::DOWNLOAD_BUFFER_BYTES];
        let mut written = 0_u64;
        let bar = ProgressBar::new(file.size).with_message(Self::name(file).to_string());
        bar.set_style(
            ProgressStyle::with_template(DisplaySettings::DOWNLOAD_TEMPLATE)
                .expect("valid template")
                .progress_chars(DisplaySettings::BAR_CHARACTERS),
        );
        if file.size < ModelSettings::DOWNLOAD_PROGRESS_FROM {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => {
                    return Err(Failure::Retry(
                        ModelSettings::WAIT_AFTER_NETWORK_ERROR,
                        format!("download interrupted: {error}"),
                    ));
                }
            };
            output.write_all(&buffer[..read]).map_err(fatal)?;
            hasher.update(&buffer[..read]);
            written += read as u64;
            bar.set_position(written);
        }
        bar.finish_and_clear();
        output.flush().map_err(fatal)?;
        if written != file.size {
            return Err(Failure::Retry(
                ModelSettings::WAIT_AFTER_NETWORK_ERROR,
                format!("{} arrived with {written} of {} bytes", Self::name(file), file.size),
            ));
        }
        let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        if digest != file.sha256 {
            return Err(Failure::Fatal(anyhow::anyhow!(
                "{} failed its SHA-256 check",
                Self::name(file)
            )));
        }
        Ok(())
    }
}
