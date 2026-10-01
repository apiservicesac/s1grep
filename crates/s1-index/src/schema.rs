use rusqlite::Connection;

use crate::error::IndexError;

/// The ordered migrations of one database. Its schema version is the number of migrations applied, kept in
/// `PRAGMA user_version`; every migration runs once, in its own transaction.
pub struct SchemaMigrations {
    /// The database's name on the connection: `main`, or the name it was attached under.
    pub database: &'static str,
    pub steps: &'static [&'static str],
}

impl SchemaMigrations {
    /// The project catalog. Version 1 is the layout every release up to 0.2.4 created without a version, so it is
    /// written with `IF NOT EXISTS` and also adopts those databases.
    pub const CATALOG: Self = Self {
        database: "main",
        steps: &[
            "
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY, hash TEXT NOT NULL, size INTEGER NOT NULL, modified INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS units (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL, name TEXT NOT NULL, start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL, source TEXT NOT NULL, content TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS units_by_path ON units(path);
            CREATE INDEX IF NOT EXISTS units_by_content ON units(content);",
            // Which extractor read each file, so a file is read again when its language's extractor changes. Every
            // file indexed before this column existed was read by the first Python extractor.
            "ALTER TABLE files ADD COLUMN extractor TEXT NOT NULL DEFAULT 'python-1';",
        ],
    };

    /// The vector cache shared by every project. The `model` column holds an embedding space key.
    pub const VECTORS: Self = Self {
        database: "shared",
        steps: &["
            CREATE TABLE IF NOT EXISTS shared.vectors (
                model TEXT NOT NULL, content TEXT NOT NULL, vector BLOB NOT NULL, PRIMARY KEY (model, content));"],
    };

    pub fn latest(&self) -> usize {
        self.steps.len()
    }

    pub fn version(&self, connection: &Connection) -> Result<usize, IndexError> {
        let version: i64 =
            connection.query_row(&format!("PRAGMA {}.user_version", self.database), [], |row| row.get(0))?;
        Ok(version as usize)
    }

    /// Applies the migrations the database has not seen. A database newer than this binary is an error: it was
    /// written by a later s1grep and must not be rewritten by an older one.
    pub fn apply(&self, connection: &mut Connection) -> Result<(), IndexError> {
        let current = self.version(connection)?;
        if current > self.latest() {
            return Err(IndexError::NewerSchema {
                database: self.database,
                found: current,
                supported: self.latest(),
            });
        }
        for (index, step) in self.steps.iter().enumerate().skip(current) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(step)?;
            transaction.execute_batch(&format!("PRAGMA {}.user_version = {}", self.database, index + 1))?;
            transaction.commit()?;
        }
        Ok(())
    }
}
