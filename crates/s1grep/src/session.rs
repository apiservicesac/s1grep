use std::path::Path;
use std::time::Instant;

use s1_index::{IndexStore, VectorIndex};

use crate::indexer::Pass;
use crate::models::Retriever;
use crate::project::Project;
use crate::settings::IndexSettings;

/// A project the background process keeps open between searches: its index connection and its vectors in memory,
/// so a search neither reopens the database nor reads every vector again (ADR-0002).
pub struct ProjectSession {
    pub project: Project,
    pub store: IndexStore,
    vectors: Option<VectorIndex>,
    /// The databases' data versions when the vectors were loaded: another process writing changes them.
    loaded_version: (i64, i64),
    last_used: Instant,
}

impl ProjectSession {
    fn open(project: &Project) -> anyhow::Result<Self> {
        let store = project.open_store()?;
        Ok(Self {
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

    /// Called after this process changed the project's units or vectors, so the next search reloads them.
    pub fn mark_changed(&mut self) {
        self.vectors = None;
    }

    /// The project's vectors, reloaded if this process or another one changed them since the last load.
    pub fn vectors(&mut self, retriever: Retriever) -> anyhow::Result<(&VectorIndex, &IndexStore)> {
        let version = self.store.data_version()?;
        if self.vectors.is_none() || version != self.loaded_version {
            let rows = self
                .store
                .vector_rows(&Pass::Whole.key(retriever), &Pass::Outline.key(retriever))?;
            self.vectors = Some(VectorIndex::new(rows));
            self.loaded_version = version;
        }
        let vectors = self.vectors.as_ref().expect("loaded just above");
        Ok((vectors, &self.store))
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
