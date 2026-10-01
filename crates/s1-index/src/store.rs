use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::IndexError;
use crate::settings::IndexLimits;
use crate::unit::CodeUnit;

/// A unit together with its row id in the index and the fingerprint of the text the retriever embeds.
#[derive(Debug, Clone)]
pub struct StoredUnit {
    pub id: i64,
    pub content: String,
    pub unit: CodeUnit,
}

/// A unit with the vector a search compares, and whether that vector comes from its whole source or only its outline.
#[derive(Debug, Clone)]
pub struct SearchableUnit {
    pub stored: StoredUnit,
    pub vector: Vec<f32>,
    pub whole: bool,
}

/// What the index knows about a file, to skip reading files that did not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub hash: String,
    pub size: u64,
    pub modified: i64,
}

/// How much of a project (or of a folder inside it) has vectors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Coverage {
    pub units: usize,
    pub embedded: usize,
}

impl Coverage {
    pub fn pending(&self) -> usize {
        self.units - self.embedded
    }

    pub fn is_complete(&self) -> bool {
        self.embedded >= self.units
    }
}

/// One project's index: its files and code units, plus a vector cache shared by every project. Vectors are keyed by
/// a fingerprint of the embedded text, so a framework copied into many projects is embedded once.
pub struct IndexStore {
    connection: Connection,
}

impl IndexStore {
    const PROJECT_SCHEMA: &'static str = "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY, hash TEXT NOT NULL, size INTEGER NOT NULL, modified INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS units (
            id INTEGER PRIMARY KEY, path TEXT NOT NULL, name TEXT NOT NULL, start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL, source TEXT NOT NULL, content TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS units_by_path ON units(path);
        CREATE INDEX IF NOT EXISTS units_by_content ON units(content);";
    const VECTOR_SCHEMA: &'static str = "
        PRAGMA shared.journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS shared.vectors (
            model TEXT NOT NULL, content TEXT NOT NULL, vector BLOB NOT NULL, PRIMARY KEY (model, content));";

    /// Opens a project index and attaches the shared vector cache.
    pub fn open(project: &Path, vectors: &Path) -> Result<Self, IndexError> {
        for path in [project, vectors] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| IndexError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }
        let connection = Connection::open(project)?;
        connection.busy_timeout(IndexLimits::BUSY_TIMEOUT)?;
        connection.execute_batch(Self::PROJECT_SCHEMA)?;
        connection.execute("ATTACH DATABASE ?1 AS shared", [vectors.to_string_lossy()])?;
        connection.execute_batch(Self::VECTOR_SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.connection
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", [key, value])?;
        Ok(())
    }

    /// Groups the writes of many files into one transaction: one write to disk instead of one per file.
    pub fn begin_batch(&self) -> Result<(), IndexError> {
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    pub fn commit_batch(&self) -> Result<(), IndexError> {
        self.connection.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn file_state(&self, path: &str) -> Result<Option<FileState>, IndexError> {
        Ok(self
            .connection
            .query_row(
                "SELECT hash, size, modified FROM files WHERE path = ?1",
                [path],
                |row| {
                    Ok(FileState {
                        hash: row.get(0)?,
                        size: row.get::<_, i64>(1)? as u64,
                        modified: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// What the index knows about every file, in one query (looking files up one by one is slow on large projects).
    pub fn file_states(&self) -> Result<std::collections::HashMap<String, FileState>, IndexError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, hash, size, modified FROM files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                FileState {
                    hash: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    modified: row.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn indexed_paths(&self) -> Result<Vec<String>, IndexError> {
        let mut statement = self.connection.prepare("SELECT path FROM files")?;
        let paths = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(paths)
    }

    /// Records a file whose content did not change but whose size or date did.
    pub fn touch_file(&self, path: &str, state: &FileState) -> Result<(), IndexError> {
        self.connection.execute(
            "UPDATE files SET size = ?2, modified = ?3 WHERE path = ?1",
            params![path, state.size as i64, state.modified],
        )?;
        Ok(())
    }

    /// Replaces everything known about one file with its new units.
    pub fn replace_file(&mut self, path: &str, state: &FileState, units: &[CodeUnit]) -> Result<(), IndexError> {
        let transaction = self.connection.savepoint()?;
        transaction.execute("DELETE FROM units WHERE path = ?1", [path])?;
        transaction.execute(
            "INSERT OR REPLACE INTO files (path, hash, size, modified) VALUES (?1, ?2, ?3, ?4)",
            params![path, state.hash, state.size as i64, state.modified],
        )?;
        for unit in units {
            transaction.execute(
                "INSERT INTO units (path, name, start_line, end_line, source, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    unit.path,
                    unit.name,
                    unit.start_line as i64,
                    unit.end_line as i64,
                    unit.source,
                    unit.content_key()
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), IndexError> {
        let transaction = self.connection.savepoint()?;
        transaction.execute("DELETE FROM units WHERE path = ?1", [path])?;
        transaction.execute("DELETE FROM files WHERE path = ?1", [path])?;
        transaction.commit()?;
        Ok(())
    }

    /// Units without a vector for `model`, those under `scope` first, one per distinct content.
    pub fn pending_units(&self, model: &str, scope: Option<&str>, limit: usize) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT MIN(id), path, name, start_line, end_line, source, content FROM units
             WHERE content NOT IN (SELECT content FROM shared.vectors WHERE model = ?1)
             GROUP BY content
             ORDER BY (?2 IS NOT NULL AND (path = ?2 OR substr(path, 1, length(?2) + 1) = ?2 || '/')) DESC, MIN(id)
             LIMIT ?3",
        )?;
        let rows = statement.query_map(params![model, scope, limit as i64], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Distinct contents under `scope` that still need a vector for `model`.
    pub fn pending_count(&self, model: &str, scope: Option<&str>) -> Result<usize, IndexError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(DISTINCT content) FROM units
             WHERE (?2 IS NULL OR path = ?2 OR substr(path, 1, length(?2) + 1) = ?2 || '/')
             AND content NOT IN (SELECT content FROM shared.vectors WHERE model = ?1)",
            params![model, scope],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn store_vectors(&mut self, model: &str, vectors: &[(String, Vec<f32>)]) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        for (content, vector) in vectors {
            let bytes: Vec<u8> = vector.iter().flat_map(|value| value.to_le_bytes()).collect();
            transaction.execute(
                "INSERT OR REPLACE INTO shared.vectors (model, content, vector) VALUES (?1, ?2, ?3)",
                params![model, content, bytes],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Units under `scope` (the whole project when `None`) that have a vector for `model`.
    pub fn units_with_vectors(
        &self,
        model: &str,
        scope: Option<&str>,
    ) -> Result<Vec<(StoredUnit, Vec<f32>)>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT units.id, path, name, start_line, end_line, source, units.content, vector FROM units
             JOIN shared.vectors AS stored ON stored.content = units.content AND stored.model = ?1
             WHERE ?2 IS NULL OR path = ?2 OR substr(path, 1, length(?2) + 1) = ?2 || '/'
             ORDER BY units.id",
        )?;
        let rows = statement.query_map(params![model, scope], |row| {
            Ok((Self::stored(row)?, Self::vector(&row.get::<_, Vec<u8>>(7)?)))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Units under `scope` with their best vector: the whole-source one from `model`, else the outline one from
    /// `outline_model`. Units with neither are left out.
    pub fn searchable_units(
        &self,
        model: &str,
        outline_model: &str,
        scope: Option<&str>,
    ) -> Result<Vec<SearchableUnit>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT units.id, path, name, start_line, end_line, source, units.content,
                    COALESCE(whole.vector, outline.vector), whole.vector IS NOT NULL FROM units
             LEFT JOIN shared.vectors AS whole ON whole.content = units.content AND whole.model = ?1
             LEFT JOIN shared.vectors AS outline ON outline.content = units.content AND outline.model = ?2
             WHERE (whole.vector IS NOT NULL OR outline.vector IS NOT NULL)
             AND (?3 IS NULL OR path = ?3 OR substr(path, 1, length(?3) + 1) = ?3 || '/')
             ORDER BY units.id",
        )?;
        let rows = statement.query_map(params![model, outline_model, scope], |row| {
            Ok(SearchableUnit {
                stored: Self::stored(row)?,
                vector: Self::vector(&row.get::<_, Vec<u8>>(7)?),
                whole: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Units under `scope`, with or without vectors, for searches that do not need them.
    pub fn units(&self, scope: Option<&str>) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT id, path, name, start_line, end_line, source, content FROM units
             WHERE ?1 IS NULL OR path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/' ORDER BY id",
        )?;
        let rows = statement.query_map(params![scope], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn coverage(&self, model: &str, scope: Option<&str>) -> Result<Coverage, IndexError> {
        let (units, embedded): (i64, i64) = self.connection.query_row(
            "SELECT COUNT(*), COUNT(stored.content) FROM units
             LEFT JOIN shared.vectors AS stored ON stored.content = units.content AND stored.model = ?1
             WHERE ?2 IS NULL OR path = ?2 OR substr(path, 1, length(?2) + 1) = ?2 || '/'",
            params![model, scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(Coverage {
            units: units as usize,
            embedded: embedded as usize,
        })
    }

    pub fn file_count(&self) -> Result<usize, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, i64>(0))? as usize)
    }

    fn vector(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }

    fn stored(row: &rusqlite::Row) -> rusqlite::Result<StoredUnit> {
        Ok(StoredUnit {
            id: row.get(0)?,
            content: row.get(6)?,
            unit: CodeUnit {
                path: row.get(1)?,
                name: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as usize,
                end_line: row.get::<_, i64>(4)? as usize,
                source: row.get(5)?,
            },
        })
    }
}
