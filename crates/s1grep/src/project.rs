use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use s1_index::IndexStore;

use crate::models::CacheDirectory;

/// The project a search belongs to: the folder whose index it uses, and the part of it the search covers.
///
/// Searching a subfolder reuses the project's index instead of building another one. The root is the highest
/// folder above the target that already has an index, otherwise the nearest one holding `.git`, otherwise the target.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    /// The searched folder relative to the root, with `/` separators; `None` for the whole project.
    pub scope: Option<String>,
}

impl Project {
    pub fn locate(path: &Path) -> anyhow::Result<Self> {
        let target = std::fs::canonicalize(path).with_context(|| format!("{} does not exist", path.display()))?;
        if !target.is_dir() {
            bail!("{} is not a folder", target.display());
        }
        let mut indexed = None;
        for ancestor in target.ancestors() {
            if CacheDirectory::project_index(ancestor)?.is_file() {
                indexed = Some(ancestor.to_path_buf());
            }
        }
        let root = indexed
            .or_else(|| {
                target
                    .ancestors()
                    .find(|ancestor| ancestor.join(".git").exists())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| target.clone());
        let relative = target.strip_prefix(&root).unwrap_or(Path::new(""));
        let scope = (!relative.as_os_str().is_empty()).then(|| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        });
        Ok(Self { root, scope })
    }

    pub fn open_store(&self) -> anyhow::Result<IndexStore> {
        let store = IndexStore::open(&CacheDirectory::project_index(&self.root)?, &CacheDirectory::vectors()?)?;
        store.set_meta("root", &self.root.to_string_lossy())?;
        Ok(store)
    }

    /// The folder the user asked for, for messages.
    pub fn target(&self) -> PathBuf {
        match &self.scope {
            Some(scope) => self.root.join(scope),
            None => self.root.clone(),
        }
    }
}

/// Only one process indexes a project at a time; the lock is released when its holder exits, even if it crashes.
pub struct IndexLock {
    file: File,
}

impl IndexLock {
    /// The lock, or the process id of the process that holds it.
    pub fn acquire(root: &Path) -> anyhow::Result<Result<Self, Option<u32>>> {
        let path = CacheDirectory::project_index(root)?.with_extension("lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                Ok(Ok(Self { file }))
            }
            Err(TryLockError::WouldBlock) => {
                let holder = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.trim().parse().ok());
                Ok(Err(holder))
            }
            Err(TryLockError::Error(error)) => Err(error).context("locking the index"),
        }
    }

    /// Whether some process is indexing `root` right now.
    pub fn is_held(root: &Path) -> bool {
        matches!(Self::acquire(root), Ok(Err(_)))
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
