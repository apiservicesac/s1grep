use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::settings::ModelSettings;

/// A model bundle s1grep needs, and where it lives on Hugging Face.
pub struct Published {
    pub bundle: &'static str,
    pub repository: &'static str,
    folder: &'static str,
}

impl Published {
    pub const ALL: [Published; 2] = [
        Published {
            bundle: ModelSettings::JUDGE_BUNDLE,
            repository: ModelSettings::JUDGE_REPOSITORY,
            folder: ModelSettings::JUDGE_FOLDER,
        },
        Published {
            bundle: ModelSettings::RETRIEVER_BUNDLE,
            repository: ModelSettings::RETRIEVER_REPOSITORY,
            folder: "",
        },
    ];
}

/// One file of a Hugging Face repository, as its tree listing describes it.
#[derive(Deserialize)]
struct RemoteFile {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: u64,
    lfs: Option<LargeFile>,
}

#[derive(Deserialize)]
struct LargeFile {
    oid: String,
}

/// Talks to the Hugging Face Hub. `HF_TOKEN` is sent when set, for repositories that are still private.
struct HubClient {
    agent: ureq::Agent,
    token: Option<String>,
}

impl HubClient {
    fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_defaults(),
            token: std::env::var("HF_TOKEN").ok().filter(|token| !token.is_empty()),
        }
    }

    fn get(&self, url: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
        let mut request = self.agent.get(url);
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        match request.call() {
            Ok(response) => Ok(response),
            Err(ureq::Error::StatusCode(status @ (401 | 403 | 404))) => {
                bail!("{url} answered {status}: the model is not public yet (set HF_TOKEN if you have access)")
            }
            Err(error) => Err(error).with_context(|| format!("downloading {url}")),
        }
    }

    fn files(&self, published: &Published) -> anyhow::Result<Vec<RemoteFile>> {
        let url = format!(
            "{}/api/models/{}/tree/main/{}",
            ModelSettings::HUB,
            published.repository,
            published.folder
        );
        let listing: Vec<RemoteFile> = self
            .get(&url)?
            .into_body()
            .read_json()
            .context("reading the file list")?;
        Ok(listing
            .into_iter()
            .filter(|file| file.kind == "file" && !file.path.ends_with(".md") && !file.path.starts_with('.'))
            .collect())
    }

    /// Streams one file to `target`, checking the SHA-256 that the Hub publishes for large files.
    fn download(&self, published: &Published, file: &RemoteFile, target: &Path) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}/resolve/main/{}",
            ModelSettings::HUB,
            published.repository,
            file.path
        );
        let partial = target.with_extension("part");
        let mut reader = self.get(&url)?.into_body().into_reader();
        let mut output = std::fs::File::create(&partial).with_context(|| format!("creating {}", partial.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 1 << 20];
        let mut written = 0_u64;
        let mut last_percent = u64::MAX;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            written += read as u64;
            let percent = written * 100 / file.size.max(1);
            if file.size > ModelSettings::DOWNLOAD_PROGRESS_FROM && percent != last_percent && percent % 10 == 0 {
                eprintln!("  {} {percent:>3} %", file.path);
                last_percent = percent;
            }
        }
        output.flush()?;
        if written != file.size {
            bail!("{} arrived with {written} bytes instead of {}", file.path, file.size);
        }
        if let Some(large) = &file.lfs {
            let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
            if digest != large.oid {
                bail!("{} failed its SHA-256 check", file.path);
            }
        }
        std::fs::rename(&partial, target)?;
        Ok(())
    }
}

/// Puts the published bundles in a model folder, skipping files that are already complete.
pub struct ModelInstaller {
    hub: HubClient,
}

impl ModelInstaller {
    pub fn new() -> Self {
        Self { hub: HubClient::new() }
    }

    pub fn install(&self, root: &Path, force: bool) -> anyhow::Result<()> {
        for published in &Published::ALL {
            let folder = root.join(published.bundle);
            std::fs::create_dir_all(&folder).with_context(|| format!("creating {}", folder.display()))?;
            for file in self.hub.files(published)? {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path).to_string();
                let target = folder.join(&name);
                let complete = target.metadata().is_ok_and(|metadata| metadata.len() == file.size);
                if complete && !force {
                    continue;
                }
                eprintln!("{} · {name} ({:.1} MB)", published.bundle, file.size as f64 / 1e6);
                self.hub.download(published, &file, &target)?;
            }
        }
        Ok(())
    }

    /// Whether every published bundle has its graph in `root`.
    pub fn is_complete(root: &Path) -> bool {
        Published::ALL
            .iter()
            .all(|published| root.join(published.bundle).join("model.onnx").is_file())
    }
}
