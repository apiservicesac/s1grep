use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::IndexError;
use crate::schema::SchemaMigrations;
use crate::settings::IndexLimits;
use crate::unit::CodeUnit;
use crate::vector_index::VectorRow;

/// A unit together with its row id in the index and the fingerprint of the text the retriever embeds.
#[derive(Debug, Clone)]
pub struct StoredUnit {
    pub id: i64,
    pub content: String,
    pub unit: CodeUnit,
}

/// What the index knows about a file, to skip reading files that did not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub hash: String,
    pub size: u64,
    pub modified: i64,
    /// Name and version of the extractor that split the file into units, e.g. `go-1`.
    pub extractor: String,
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

/// The SQL condition "this unit's path is the folder bound to parameter `?N` or inside it", written once.
struct ScopeFilter;

impl ScopeFilter {
    fn matches(parameter: usize) -> String {
        format!("(path = ?{parameter} OR substr(path, 1, length(?{parameter}) + 1) = ?{parameter} || '/')")
    }

    /// "This unit has no vector in the space bound to `?N`": an index lookup per unit, unlike `NOT IN`, which would
    /// list every vector of the space across all projects.
    fn missing_vector(parameter: usize) -> String {
        format!(
            "NOT EXISTS (SELECT 1 FROM shared.vectors AS present
                         WHERE present.model = ?{parameter} AND present.content = units.content)"
        )
    }
}

/// One project's index: its files and code units, plus a vector cache shared by every project. Vectors are keyed by
/// their embedding space and a fingerprint of the unit, so a framework copied into many projects is embedded once.
pub struct IndexStore {
    connection: Connection,
}

impl IndexStore {
    /// Opens a project's catalog and attaches the shared vector cache, bringing both schemas up to date.
    pub fn open(catalog: &Path, vectors: &Path) -> Result<Self, IndexError> {
        for path in [catalog, vectors] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| IndexError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }
        let mut connection = Connection::open(catalog)?;
        connection.busy_timeout(IndexLimits::BUSY_TIMEOUT)?;
        connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        connection.execute("ATTACH DATABASE ?1 AS shared", [vectors.to_string_lossy()])?;
        connection.execute_batch("PRAGMA shared.journal_mode = WAL; PRAGMA shared.synchronous = NORMAL;")?;
        SchemaMigrations::CATALOG.apply(&mut connection)?;
        SchemaMigrations::VECTORS.apply(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    /// Folds the write-ahead log into the database file, so the database is complete in one file.
    pub fn fold_write_ahead_log(&self) -> Result<(), IndexError> {
        self.connection.execute_batch("PRAGMA main.wal_checkpoint(TRUNCATE);")?;
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

    /// What the index knows about every file, in one query (looking files up one by one is slow on large projects).
    pub fn file_states(&self) -> Result<std::collections::HashMap<String, FileState>, IndexError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, hash, size, modified, extractor FROM files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                FileState {
                    hash: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    modified: row.get(3)?,
                    extractor: row.get(4)?,
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
            "INSERT OR REPLACE INTO files (path, hash, size, modified, extractor) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![path, state.hash, state.size as i64, state.modified, state.extractor],
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

    /// Up to `limit` distinct contents without a vector in `space`, only those under `scope`.
    pub fn pending_units_within(
        &self,
        space: &str,
        scope: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT MIN(id), path, name, start_line, end_line, source, content FROM units
             WHERE (?2 IS NULL OR {}) AND {}
             GROUP BY content ORDER BY MIN(id) LIMIT ?3",
            ScopeFilter::matches(2),
            ScopeFilter::missing_vector(1)
        ))?;
        let rows = statement.query_map(params![space, scope, limit as i64], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Up to `limit` distinct contents without a vector in `space` anywhere in the project, those with a copy under
    /// `scope` first.
    pub fn pending_units_scope_first(
        &self,
        space: &str,
        scope: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT MIN(id), path, name, start_line, end_line, source, content FROM units
             WHERE {}
             GROUP BY content
             ORDER BY MAX(?2 IS NOT NULL AND {}) DESC, MIN(id)
             LIMIT ?3",
            ScopeFilter::missing_vector(1),
            ScopeFilter::matches(2)
        ))?;
        let rows = statement.query_map(params![space, scope, limit as i64], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Distinct contents under `scope` that still need a vector in `space`.
    pub fn pending_count(&self, space: &str, scope: Option<&str>) -> Result<usize, IndexError> {
        let count: i64 = self.connection.query_row(
            &format!(
                "SELECT COUNT(DISTINCT content) FROM units WHERE (?2 IS NULL OR {}) AND {}",
                ScopeFilter::matches(2),
                ScopeFilter::missing_vector(1)
            ),
            params![space, scope],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Every content key this project's units use, so that a clean-up keeps their vectors.
    pub fn content_keys(&self) -> Result<HashSet<String>, IndexError> {
        let mut statement = self.connection.prepare("SELECT DISTINCT content FROM units")?;
        let keys = statement.query_map([], |row| row.get(0))?;
        Ok(keys.collect::<Result<_, _>>()?)
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

    /// Every unit of the project with a vector in `space`, except those that also have one in `except_space`
    /// (outlines are searched only for functions whose whole source has no vector yet). No sources are read: they are
    /// fetched for the final candidates only.
    pub fn vector_rows(
        &self,
        space: &str,
        except_space: Option<&str>,
    ) -> Result<Vec<(VectorRow, Vec<f32>)>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT units.id, path, stored.vector FROM units
             JOIN shared.vectors AS stored ON stored.content = units.content AND stored.model = ?1
             WHERE ?2 IS NULL OR NOT EXISTS (SELECT 1 FROM shared.vectors AS other
                                             WHERE other.model = ?2 AND other.content = units.content)
             ORDER BY units.id",
        )?;
        let whole = except_space.is_none();
        let rows = statement.query_map(params![space, except_space], |row| {
            Ok((
                VectorRow {
                    unit_id: row.get(0)?,
                    path: row.get(1)?,
                    whole,
                },
                Self::vector(&row.get::<_, Vec<u8>>(2)?),
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every unit of the project, for rebuilding an index kept beside the catalog.
    pub fn all_units(&self) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, path, name, start_line, end_line, source, content FROM units ORDER BY id")?;
        let rows = statement.query_map([], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The units of these files.
    pub fn units_in_files(&self, paths: &[String]) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id, path, name, start_line, end_line, source, content FROM units WHERE path = ?1 ORDER BY id",
        )?;
        let mut units = Vec::new();
        for path in paths {
            let rows = statement.query_map([path], Self::stored)?;
            for unit in rows {
                units.push(unit?);
            }
        }
        Ok(units)
    }

    /// The units with these ids, in the same order; ids no longer in the index are left out.
    pub fn units_by_ids(&self, ids: &[i64]) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT id, path, name, start_line, end_line, source, content FROM units WHERE id = ?1")?;
        let mut units = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(unit) = statement.query_row([id], Self::stored).optional()? {
                units.push(unit);
            }
        }
        Ok(units)
    }

    /// Changes whenever another connection commits to the catalog or the vector cache, so a copy kept in memory
    /// knows when to reload.
    pub fn data_version(&self) -> Result<(i64, i64), IndexError> {
        let catalog = self
            .connection
            .query_row("PRAGMA main.data_version", [], |row| row.get(0))?;
        let vectors = self
            .connection
            .query_row("PRAGMA shared.data_version", [], |row| row.get(0))?;
        Ok((catalog, vectors))
    }

    /// Units under `scope` that can be searched: with a vector in `space` or in `outline_space`.
    pub fn searchable_count(&self, space: &str, outline_space: &str, scope: Option<&str>) -> Result<usize, IndexError> {
        let count: i64 = self.connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM units WHERE (?3 IS NULL OR {})
                 AND (EXISTS (SELECT 1 FROM shared.vectors AS whole WHERE whole.model = ?1 AND whole.content = units.content)
                   OR EXISTS (SELECT 1 FROM shared.vectors AS outline
                              WHERE outline.model = ?2 AND outline.content = units.content))",
                ScopeFilter::matches(3)
            ),
            params![space, outline_space, scope],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn unit_count(&self) -> Result<usize, IndexError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM units", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn coverage(&self, model: &str, scope: Option<&str>) -> Result<Coverage, IndexError> {
        let (units, embedded): (i64, i64) = self.connection.query_row(
            &format!(
                "SELECT COUNT(*), COUNT(stored.content) FROM units
                 LEFT JOIN shared.vectors AS stored ON stored.content = units.content AND stored.model = ?1
                 WHERE ?2 IS NULL OR {}",
                ScopeFilter::matches(2)
            ),
            params![model, scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(Coverage {
            units: units as usize,
            embedded: embedded as usize,
        })
    }

    fn vector(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
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

/// The shared vector cache on its own, for clean-ups that span every project.
pub struct VectorCache {
    connection: Connection,
}

impl VectorCache {
    pub fn open(vectors: &Path) -> Result<Self, IndexError> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute("ATTACH DATABASE ?1 AS shared", [vectors.to_string_lossy()])?;
        connection.busy_timeout(IndexLimits::BUSY_TIMEOUT)?;
        SchemaMigrations::VECTORS.apply(&mut connection)?;
        Ok(Self { connection })
    }

    /// Moves the vectors stored under an old space key to its current key, keeping any already stored there.
    pub fn rename_space(&mut self, from: &str, to: &str) -> Result<usize, IndexError> {
        let transaction = self.connection.transaction()?;
        let moved = transaction.execute(
            "UPDATE OR IGNORE shared.vectors SET model = ?2 WHERE model = ?1",
            params![from, to],
        )?;
        transaction.execute("DELETE FROM shared.vectors WHERE model = ?1", [from])?;
        transaction.commit()?;
        Ok(moved)
    }

    /// Deletes vectors of spaces not in `spaces` and of contents no project uses, then compacts the file. Returns how
    /// many vectors were removed.
    pub fn remove_unused(&mut self, spaces: &[String], contents: &HashSet<String>) -> Result<usize, IndexError> {
        let transaction = self.connection.transaction()?;
        transaction.execute_batch(
            "CREATE TEMP TABLE kept_spaces (model TEXT PRIMARY KEY);
                                   CREATE TEMP TABLE kept_contents (content TEXT PRIMARY KEY);",
        )?;
        for space in spaces {
            transaction.execute("INSERT OR IGNORE INTO kept_spaces VALUES (?1)", [space])?;
        }
        for content in contents {
            transaction.execute("INSERT OR IGNORE INTO kept_contents VALUES (?1)", [content])?;
        }
        let removed = transaction.execute(
            "DELETE FROM shared.vectors
             WHERE model NOT IN (SELECT model FROM kept_spaces)
             OR content NOT IN (SELECT content FROM kept_contents)",
            [],
        )?;
        transaction.execute_batch("DROP TABLE kept_spaces; DROP TABLE kept_contents;")?;
        transaction.commit()?;
        self.connection.execute_batch("VACUUM shared;")?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::Connection;

    use super::IndexStore;
    use crate::error::IndexError;
    use crate::store::FileState;
    use crate::unit::CodeUnit;

    /// A folder under the system temporary directory, removed when dropped.
    struct ScratchFolder {
        path: PathBuf,
    }

    impl ScratchFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("s1-index-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn unit(path: &str, name: &str) -> CodeUnit {
        CodeUnit {
            path: path.to_string(),
            name: name.to_string(),
            start_line: 1,
            end_line: 3,
            source: format!("def {name}():\n    value = 1\n    return value"),
        }
    }

    fn state() -> FileState {
        FileState {
            hash: "hash".to_string(),
            size: 1,
            modified: 1,
            extractor: "python-1".to_string(),
        }
    }

    #[test]
    fn adopts_an_unversioned_catalog_and_refuses_a_newer_one() {
        let folder = ScratchFolder::new("schema");
        let catalog = folder.path.join("catalog.sqlite");
        let vectors = folder.path.join("vectors.sqlite");
        Connection::open(&catalog)
            .unwrap()
            .execute_batch("CREATE TABLE files (path TEXT PRIMARY KEY, hash TEXT NOT NULL, size INTEGER NOT NULL, modified INTEGER NOT NULL);")
            .unwrap();
        drop(IndexStore::open(&catalog, &vectors).unwrap());
        let version: i64 = Connection::open(&catalog)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        Connection::open(&catalog)
            .unwrap()
            .execute_batch("PRAGMA user_version = 99")
            .unwrap();
        assert!(matches!(
            IndexStore::open(&catalog, &vectors),
            Err(IndexError::NewerSchema { database: "main", .. })
        ));
    }

    #[test]
    fn a_scoped_pass_finds_a_function_whose_first_copy_is_outside_the_scope() {
        let folder = ScratchFolder::new("scope");
        let mut store =
            IndexStore::open(&folder.path.join("catalog.sqlite"), &folder.path.join("vectors.sqlite")).unwrap();
        store
            .replace_file("vendor/a.py", &state(), &[unit("vendor/a.py", "shared")])
            .unwrap();
        store
            .replace_file("app/b.py", &state(), &[unit("app/b.py", "shared")])
            .unwrap();
        assert_eq!(store.pending_count("space", Some("app")).unwrap(), 1);
        assert_eq!(store.pending_units_within("space", Some("app"), 10).unwrap().len(), 1);
        let content = store.pending_units_within("space", Some("app"), 10).unwrap()[0]
            .content
            .clone();
        store.store_vectors("space", &[(content, vec![1.0])]).unwrap();
        assert_eq!(store.pending_count("space", Some("app")).unwrap(), 0);
        assert_eq!(store.pending_count("space", None).unwrap(), 0);
    }

    #[test]
    fn renaming_a_space_keeps_vectors_already_under_the_new_key() {
        let folder = ScratchFolder::new("rename");
        let vectors = folder.path.join("vectors.sqlite");
        let mut store = IndexStore::open(&folder.path.join("catalog.sqlite"), &vectors).unwrap();
        store
            .replace_file("a.py", &state(), &[unit("a.py", "first"), unit("a.py", "second")])
            .unwrap();
        let pending = store.pending_units_within("old", None, 10).unwrap();
        store
            .store_vectors(
                "old",
                &[
                    (pending[0].content.clone(), vec![1.0]),
                    (pending[1].content.clone(), vec![2.0]),
                ],
            )
            .unwrap();
        store
            .store_vectors("new", &[(pending[0].content.clone(), vec![3.0])])
            .unwrap();
        super::VectorCache::open(&vectors)
            .unwrap()
            .rename_space("old", "new")
            .unwrap();
        assert_eq!(store.pending_count("new", None).unwrap(), 0);
        assert_eq!(store.pending_count("old", None).unwrap(), 2);
        let rows = store.vector_rows("new", None).unwrap();
        assert_eq!(rows[0].1, vec![3.0]);
        assert_eq!(rows[1].1, vec![2.0]);
    }
}
