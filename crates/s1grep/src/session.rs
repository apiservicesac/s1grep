use std::path::Path;
use std::time::Instant;

use s1_index::{IndexStore, LexicalIndex, VectorIndex};

use crate::searcher::SearchIndexes;

use crate::indexer::{Pass, ScanReport};
use crate::models::Retriever;
use crate::project::Project;
use crate::settings::IndexSettings;

/// A project the background process keeps open between searches: its index connection and its vectors in memory,
/// so a search neither reopens the database nor reads every vector again (ADR-0002).
pub struct ProjectSession {
    pub project: Project,
    pub store: IndexStore,
    vectors: Option<SearchIndexes>,
    /// The project's word index; `None` if it cannot be opened or written (another process holds its writer), in
    /// which case searches go on by meaning alone.
    lexical: Option<LexicalIndex>,
    /// The databases' data versions when the vectors were loaded: another process writing changes them.
    loaded_version: (i64, i64),
    last_used: Instant,
}

impl ProjectSession {
    fn open(project: &Project) -> anyhow::Result<Self> {
        let store = project.open_store()?;
        let lexical = Self::open_lexical(project, &store);
        Ok(Self {
            lexical,
            project: Project {
                scope: None,
                ..project.clone()
            },
            loaded_version: store.data_version()?,
            store,
            vectors: None,
            last_used: Instant::now(),
        })
    }

    /// The word index, rebuilt from the catalog when it does not hold the same number of functions (a new project, an
    /// older format, a write interrupted between the catalog and the index).
    fn open_lexical(project: &Project, store: &IndexStore) -> Option<LexicalIndex> {
        let opened = LexicalIndex::open(&project.folder.lexical()).and_then(|mut index| {
            if index.document_count() != store.unit_count()? as u64 {
                index.rebuild(&store.all_units()?)?;
            }
            Ok(index)
        });
        match opened {
            Ok(index) => Some(index),
            Err(error) => {
                eprintln!(
                    "s1grep: searching {} without its word index: {error}",
                    project.root.display()
                );
                None
            }
        }
    }

    /// Brings the word index up to date with a scan's changed files.
    pub fn apply_scan(&mut self, report: &ScanReport) {
        let Some(lexical) = self.lexical.as_mut() else {
            return;
        };
        if report.changed_paths.is_empty() {
            return;
        }
        let updated = self
            .store
            .units_in_files(&report.changed_paths)
            .map_err(anyhow::Error::from)
            .and_then(|units| Ok(lexical.replace_files(&report.changed_paths, &units)?));
        if let Err(error) = updated {
            eprintln!("s1grep: word index left behind, rebuilt on next open: {error}");
            self.lexical = None;
        }
    }

    /// Called after this process changed the project's units or vectors, so the next search reloads them.
    pub fn mark_changed(&mut self) {
        self.vectors = None;
    }

    /// The project's vectors, reloaded if this process or another one changed them since the last load.
    pub fn vectors(
        &mut self,
        retriever: Retriever,
    ) -> anyhow::Result<(&SearchIndexes, Option<&LexicalIndex>, &IndexStore)> {
        let version = self.store.data_version()?;
        if self.vectors.is_none() || version != self.loaded_version {
            let whole = Pass::Whole.key(retriever);
            let outline = Pass::Outline.key(retriever);
            self.vectors = Some(SearchIndexes {
                whole: VectorIndex::new(self.store.vector_rows(&whole, None)?),
                outline: VectorIndex::new(self.store.vector_rows(&outline, Some(&whole))?),
            });
            self.loaded_version = version;
        }
        let vectors = self.vectors.as_ref().expect("loaded just above");
        Ok((vectors, self.lexical.as_ref(), &self.store))
    }
}

/// The projects kept open, the least recently used closed first beyond `IndexSettings::OPEN_PROJECTS`.
#[derive(Default)]
pub struct ProjectSessions {
    sessions: Vec<ProjectSession>,
}

impl ProjectSessions {
    pub fn get(&mut self, project: &Project) -> anyhow::Result<&mut ProjectSession> {
        let position = match self.position(&project.root) {
            Some(position) => position,
            None => {
                if self.sessions.len() >= IndexSettings::OPEN_PROJECTS
                    && let Some(oldest) = self
                        .sessions
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, session)| session.last_used)
                        .map(|(position, _)| position)
                {
                    self.sessions.remove(oldest);
                }
                self.sessions.push(ProjectSession::open(project)?);
                self.sessions.len() - 1
            }
        };
        let session = &mut self.sessions[position];
        session.last_used = Instant::now();
        Ok(session)
    }

    fn position(&self, root: &Path) -> Option<usize> {
        self.sessions.iter().position(|session| session.project.root == root)
    }
}
