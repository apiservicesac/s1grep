use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use s1_index::{IndexError, IndexStore};

use crate::cache::{CacheDirectory, ProjectFolder};
use crate::settings::CacheSettings;

/// The project a search belongs to: the folder whose index it uses, and the part of it the search covers.
///
/// Searching a subfolder reuses the project's index instead of building another one. The root is the highest
/// folder above the target that already has an index, otherwise the nearest one holding `.git`, otherwise the target.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    /// The searched folder relative to the root, with `/` separators; `None` for the whole project.
    pub scope: Option<String>,
    pub folder: ProjectFolder,
}

impl Project {
    pub fn locate(path: &Path) -> anyhow::Result<Self> {
        let target = dunce::canonicalize(path).with_context(|| format!("{} does not exist", path.display()))?;
        if !target.is_dir() {
            bail!("{} is not a folder", target.display());
        }
        let mut indexed = None;
        for ancestor in target.ancestors() {
            if ProjectFolder::exists_for(ancestor)? {
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
        let folder = ProjectFolder::for_root(&root)?;
        Ok(Self { root, scope, folder })
    }

    /// The project rooted exactly at `root`, without looking for an enclosing index or repository.
    pub fn at_root(root: &Path) -> anyhow::Result<Self> {
        let root = dunce::canonicalize(root).with_context(|| format!("{} does not exist", root.display()))?;
        let folder = ProjectFolder::for_root(&root)?;
        Ok(Self {
            root,
            scope: None,
            folder,
        })
    }

    /// Records that the project was searched now, for `status` and the clean-up of unused indexes.
    pub fn record_use(&self) -> anyhow::Result<()> {
        self.folder.record_use(&self.root)
    }

    /// Opens the project's index, creating or migrating it. A catalog written
    /// by a newer s1grep is rebuilt: it holds only what a scan recreates, while vectors live in the shared cache.
    pub fn open_store(&self) -> anyhow::Result<IndexStore> {
        let vectors = CacheDirectory::vectors()?;
        let store = match IndexStore::open(&self.folder.catalog(), &vectors) {
            Err(IndexError::NewerSchema { database: "main", .. }) => {
                for suffix in std::iter::once("").chain(CacheSettings::SQLITE_SIDE_FILES) {
                    let _ = std::fs::remove_file(format!("{}{suffix}", self.folder.catalog().display()));
                }
                IndexStore::open(&self.folder.catalog(), &vectors)?
            }
            other => other?,
        };
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
    pub fn acquire(folder: &ProjectFolder) -> anyhow::Result<Result<Self, Option<u32>>> {
        std::fs::create_dir_all(&folder.path).with_context(|| format!("creating {}", folder.path.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.lock_file())?;
        match file.try_lock() {
            Ok(()) => {
                std::fs::write(folder.holder_file(), std::process::id().to_string())?;
                Ok(Ok(Self { file }))
            }
            Err(TryLockError::WouldBlock) => Ok(Err(Self::holder(folder))),
            Err(TryLockError::Error(error)) => Err(error).context("locking the index"),
        }
    }

    /// Whether some process is indexing the project right now. It only asks for a shared lock, and only for an
    /// instant, so it never records itself as the holder.
    pub fn is_held(folder: &ProjectFolder) -> bool {
        let Ok(file) = File::open(folder.lock_file()) else {
            return false;
        };
        match file.try_lock_shared() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            Err(_) => true,
        }
    }

    fn holder(folder: &ProjectFolder) -> Option<u32> {
        std::fs::read_to_string(folder.holder_file())
            .ok()
            .and_then(|text| text.trim().parse().ok())
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
