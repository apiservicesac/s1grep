use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use s1_index::VectorCache;

use crate::indexer::Pass;
use crate::models::Retriever;
use crate::settings::{CacheSettings, ModelSettings};

/// The per-user cache: `$XDG_CACHE_HOME/s1grep`, or `~/.cache/s1grep` (`%LOCALAPPDATA%\s1grep` on Windows).
pub struct CacheDirectory;

impl CacheDirectory {
    pub fn root() -> anyhow::Result<PathBuf> {
        if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(cache).join(CacheSettings::APPLICATION_FOLDER));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local).join(CacheSettings::APPLICATION_FOLDER));
        }
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home)
            .join(".cache")
            .join(CacheSettings::APPLICATION_FOLDER))
    }

    pub fn models() -> anyhow::Result<PathBuf> {
        Ok(Self::root()?.join(CacheSettings::MODELS))
    }

    /// Vectors shared by every project, keyed by embedding space and content. The first call in a process moves
    /// vectors stored under the keys of s1grep 0.2.4 and earlier to their embedding spaces.
    pub fn vectors() -> anyhow::Result<PathBuf> {
        static ADOPTED: OnceLock<()> = OnceLock::new();
        let path = Self::root()?.join(CacheSettings::VECTORS);
        if ADOPTED.get().is_none() && path.is_file() {
            let mut cache = VectorCache::open(&path)?;
            let retriever = Retriever::Granite;
            cache.rename_space(ModelSettings::LEGACY_WHOLE_SPACE, &Pass::Whole.key(retriever))?;
            cache.rename_space(ModelSettings::LEGACY_OUTLINE_SPACE, &Pass::Outline.key(retriever))?;
            let _ = ADOPTED.set(());
        }
        Ok(path)
    }

    pub fn projects() -> anyhow::Result<PathBuf> {
        Ok(Self::root()?.join(CacheSettings::PROJECTS))
    }
}

/// Writes a file so that readers see either the old content or the new one, never half of it.
pub struct AtomicFile;

impl AtomicFile {
    pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))
    }
}

/// What a project folder records about the project it indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub root: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub last_used: u64,
}

impl ProjectInfo {
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

/// One project's place in the cache: `projects/<fingerprint of its path>/` with `project.json`, `catalog.sqlite`
/// and the indexing lock. Indexes written by s1grep 0.2.4 and earlier (`projects/<key>.sqlite` and `.lock`) move in
/// here the first time they are seen.
#[derive(Debug, Clone)]
pub struct ProjectFolder {
    pub path: PathBuf,
}

impl ProjectFolder {
    pub fn for_root(root: &Path) -> anyhow::Result<Self> {
        let folder = Self {
            path: CacheDirectory::projects()?.join(Self::key(root)),
        };
        for legacy_key in Self::legacy_keys(root) {
            folder.adopt_legacy_index(&legacy_key)?;
        }
        Ok(folder)
    }

    /// Whether `root` already has an index, in this layout or the previous one.
    pub fn exists_for(root: &Path) -> anyhow::Result<bool> {
        let projects = CacheDirectory::projects()?;
        Ok(projects.join(Self::key(root)).join(CacheSettings::CATALOG).is_file()
            || Self::legacy_keys(root).iter().any(|key| {
                projects
                    .join(format!("{key}.{}", CacheSettings::LEGACY_INDEX_EXTENSION))
                    .is_file()
            }))
    }

    /// Every project folder in the cache, after moving any index of the previous layout into its folder.
    pub fn all() -> anyhow::Result<Vec<Self>> {
        let projects = CacheDirectory::projects()?;
        let Ok(entries) = std::fs::read_dir(&projects) else {
            return Ok(Vec::new());
        };
        let entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
        for legacy in entries.iter().filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == CacheSettings::LEGACY_INDEX_EXTENSION)
        }) {
            if let Some(key) = legacy.file_stem() {
                let key = key.to_string_lossy();
                Self {
                    path: projects.join(key.as_ref()),
                }
                .adopt_legacy_index(&key)?;
            }
        }
        let mut folders: Vec<Self> = std::fs::read_dir(&projects)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join(CacheSettings::CATALOG).is_file())
            .map(|path| Self { path })
            .collect();
        folders.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(folders)
    }

    pub fn catalog(&self) -> PathBuf {
        self.path.join(CacheSettings::CATALOG)
    }

    pub fn lock_file(&self) -> PathBuf {
        self.path.join(CacheSettings::LOCK)
    }

    /// The process id of whoever holds the lock, kept apart from the lock file: Windows locks bar reading it.
    pub fn holder_file(&self) -> PathBuf {
        self.path.join(CacheSettings::LOCK_HOLDER)
    }

    pub fn info(&self) -> Option<ProjectInfo> {
        let text = std::fs::read_to_string(self.path.join(CacheSettings::PROJECT_INFO)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Records that `root` was searched now, creating `project.json` the first time.
    pub fn record_use(&self, root: &Path) -> anyhow::Result<()> {
        let now = ProjectInfo::now();
        let info = match self.info() {
            Some(info) => ProjectInfo { last_used: now, ..info },
            None => ProjectInfo {
                root: root.to_string_lossy().to_string(),
                created: now,
                last_used: now,
            },
        };
        AtomicFile::write(
            &self.path.join(CacheSettings::PROJECT_INFO),
            serde_json::to_string_pretty(&info)?.as_bytes(),
        )
    }

    /// Deletes the whole folder; the caller holds the project's lock.
    pub fn remove(&self) -> anyhow::Result<()> {
        std::fs::remove_dir_all(&self.path).with_context(|| format!("removing {}", self.path.display()))
    }

    fn key(root: &Path) -> String {
        Self::key_of(&root.to_string_lossy())
    }

    fn key_of(path: &str) -> String {
        let fingerprint = s1_index::ContentFingerprint::of(path.as_bytes());
        fingerprint[..CacheSettings::PROJECT_KEY_LENGTH].to_string()
    }

    /// Keys an index of this root may have had in the previous layout: the same key and, on Windows, the key of the
    /// `\\?\` path that canonicalisation used to return.
    fn legacy_keys(root: &Path) -> Vec<String> {
        let mut keys = vec![Self::key(root)];
        if cfg!(windows) {
            keys.push(Self::key_of(&format!(r"\\?\{}", root.to_string_lossy())));
        }
        keys
    }

    /// Moves `projects/<key>.sqlite` into this folder as `catalog.sqlite`, writes `project.json` from the root it
    /// recorded, and drops the old lock file. The write-ahead log is folded into the database first, so only one file
    /// moves, and a rename cannot lose an index even if two processes do this at once.
    fn adopt_legacy_index(&self, key: &str) -> anyhow::Result<()> {
        let Some(projects) = self.path.parent() else {
            return Ok(());
        };
        let legacy = projects.join(format!("{key}.{}", CacheSettings::LEGACY_INDEX_EXTENSION));
        if !legacy.is_file() || self.catalog().exists() {
            return Ok(());
        }
        let root = {
            let store = s1_index::IndexStore::open(&legacy, &CacheDirectory::vectors()?)?;
            store.fold_write_ahead_log()?;
            store.meta("root")?
        };
        std::fs::create_dir_all(&self.path).with_context(|| format!("creating {}", self.path.display()))?;
        std::fs::rename(&legacy, self.catalog()).with_context(|| format!("moving {}", legacy.display()))?;
        for suffix in CacheSettings::SQLITE_SIDE_FILES {
            let _ = std::fs::remove_file(format!("{}{suffix}", legacy.display()));
        }
        let _ = std::fs::remove_file(projects.join(format!("{key}.{}", CacheSettings::LEGACY_LOCK_EXTENSION)));
        if let Some(root) = root {
            self.record_use(Path::new(&root))?;
        }
        Ok(())
    }
}
