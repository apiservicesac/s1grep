use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::IndexError;
use crate::unit::CodeUnit;

/// A unit together with its row id in the index.
#[derive(Debug, Clone)]
pub struct StoredUnit {
    pub id: i64,
    pub unit: CodeUnit,
}

/// The on-disk index: files with their content hash, the code units they contain and one vector per unit and model.
/// Re-indexing only touches files whose hash changed, so a second run over an unchanged repository is instant.
pub struct IndexStore {
    connection: Connection,
}

impl IndexStore {
    const SCHEMA: &'static str = "
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS files (path TEXT PRIMARY KEY, hash TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS units (
            id INTEGER PRIMARY KEY, path TEXT NOT NULL, name TEXT NOT NULL,
            start_line INTEGER NOT NULL, end_line INTEGER NOT NULL, source TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS units_by_path ON units(path);
        CREATE TABLE IF NOT EXISTS vectors (
            unit_id INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE, model TEXT NOT NULL, vector BLOB NOT NULL,
            PRIMARY KEY (unit_id, model));
        PRAGMA foreign_keys = ON;";

    pub fn open(path: &Path) -> Result<Self, IndexError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::Io { path: parent.to_path_buf(), source })?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(Self::SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn file_hash(&self, path: &str) -> Result<Option<String>, IndexError> {
        Ok(self.connection.query_row("SELECT hash FROM files WHERE path = ?1", [path], |row| row.get(0)).optional()?)
    }

    pub fn indexed_paths(&self) -> Result<Vec<String>, IndexError> {
        let mut statement = self.connection.prepare("SELECT path FROM files")?;
        let paths = statement.query_map([], |row| row.get(0))?.collect::<Result<Vec<String>, _>>()?;
        Ok(paths)
    }

    /// Replaces everything known about one file with its new units.
    pub fn replace_file(&mut self, path: &str, hash: &str, units: &[CodeUnit]) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM units WHERE path = ?1", [path])?;
        transaction.execute("INSERT OR REPLACE INTO files (path, hash) VALUES (?1, ?2)", [path, hash])?;
        for unit in units {
            transaction.execute(
                "INSERT INTO units (path, name, start_line, end_line, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![unit.path, unit.name, unit.start_line as i64, unit.end_line as i64, unit.source],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM units WHERE path = ?1", [path])?;
        transaction.execute("DELETE FROM files WHERE path = ?1", [path])?;
        transaction.commit()?;
        Ok(())
    }

    /// Units that still have no vector for `model`.
    pub fn units_without_vector(&self, model: &str) -> Result<Vec<StoredUnit>, IndexError> {
        self.units_where("WHERE id NOT IN (SELECT unit_id FROM vectors WHERE model = ?1)", model)
    }

    pub fn store_vectors(&mut self, model: &str, vectors: &[(i64, Vec<f32>)]) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        for (unit_id, vector) in vectors {
            let bytes: Vec<u8> = vector.iter().flat_map(|value| value.to_le_bytes()).collect();
            transaction.execute(
                "INSERT OR REPLACE INTO vectors (unit_id, model, vector) VALUES (?1, ?2, ?3)",
                params![unit_id, model, bytes],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Every unit with its vector for `model`, ready for a brute-force search.
    pub fn units_with_vectors(&self, model: &str) -> Result<Vec<(StoredUnit, Vec<f32>)>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT units.id, path, name, start_line, end_line, source, vector FROM units
             JOIN vectors ON vectors.unit_id = units.id AND vectors.model = ?1 ORDER BY units.id",
        )?;
        let rows = statement.query_map([model], |row| {
            let bytes: Vec<u8> = row.get(6)?;
            let vector = bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            Ok((Self::stored(row)?, vector.collect()))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn unit_count(&self) -> Result<usize, IndexError> {
        Ok(self.connection.query_row("SELECT COUNT(*) FROM units", [], |row| row.get::<_, i64>(0))? as usize)
    }

    fn units_where(&self, clause: &str, model: &str) -> Result<Vec<StoredUnit>, IndexError> {
        let mut statement = self
            .connection
            .prepare(&format!("SELECT id, path, name, start_line, end_line, source FROM units {clause} ORDER BY id"))?;
        let rows = statement.query_map([model], Self::stored)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn stored(row: &rusqlite::Row) -> rusqlite::Result<StoredUnit> {
        Ok(StoredUnit {
            id: row.get(0)?,
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
