use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::models::{CacheDirectory, ModelDirectory, Retriever};

/// A model bundle s1grep needs, and where it lives on Hugging Face.
struct Published {
    bundle: &'static str,
    repository: &'static str,
    folder: &'static str,
}

impl Published {
    const ALL: [Published; 2] = [
        Published {
            bundle: ModelDirectory::JUDGE_BUNDLE,
            repository: "api-service-sac/s1-code-v3",
            folder: "onnx",
        },
        Published {
            bundle: "granite-278m-onnx",
            repository: "api-service-sac/granite-embedding-278m-multilingual-onnx",
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
    const HOST: &'static str = "https://huggingface.co";

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
            Self::HOST,
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
        let url = format!("{}/{}/resolve/main/{}", Self::HOST, published.repository, file.path);
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
            if file.size > Self::PROGRESS_FROM && percent != last_percent && percent % 10 == 0 {
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

    const PROGRESS_FROM: u64 = 50_000_000;
}

#[derive(Subcommand)]
enum ModelsAction {
    /// Download the models s1grep needs (about 2.4 GB) into the model folder
    Download {
        /// Download again even when a file is already there
        #[arg(long)]
        force: bool,
    },
    /// Show where the models are and whether each one is present
    Status,
}

#[derive(Args)]
pub struct ModelsCommand {
    #[command(subcommand)]
    action: ModelsAction,
    #[command(flatten)]
    models: ModelDirectory,
}

impl ModelsCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let root = self.root()?;
        match self.action {
            ModelsAction::Download { force } => Self::download(&root, force),
            ModelsAction::Status => {
                println!("{}", root.display());
                for published in &Published::ALL {
                    let present = root.join(published.bundle).join("model.onnx").is_file();
                    println!(
                        "  {:<22} {}",
                        published.bundle,
                        if present {
                            "ready"
                        } else {
                            "missing (run: s1grep models download)"
                        }
                    );
                }
                Ok(())
            }
        }
    }

    fn root(&self) -> anyhow::Result<PathBuf> {
        match self.models.root() {
            Some(root) => Ok(root.to_path_buf()),
            None => Ok(CacheDirectory::root()?.join("models")),
        }
    }

    fn download(root: &Path, force: bool) -> anyhow::Result<()> {
        let hub = HubClient::new();
        for published in &Published::ALL {
            let folder = root.join(published.bundle);
            std::fs::create_dir_all(&folder).with_context(|| format!("creating {}", folder.display()))?;
            for file in hub.files(published)? {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path).to_string();
                let target = folder.join(&name);
                let complete = target.metadata().is_ok_and(|metadata| metadata.len() == file.size);
                if complete && !force {
                    continue;
                }
                eprintln!("{} · {name} ({:.1} MB)", published.bundle, file.size as f64 / 1e6);
                hub.download(published, &file, &target)?;
            }
        }
        eprintln!(
            "models ready in {} (retriever: {})",
            root.display(),
            Retriever::Granite.bundle_name()
        );
        Ok(())
    }
}
