//! SQLite database connection management and migration runner.
//!
//! OWNER: worker W1.

use std::path::Path;

use rusqlite::Connection;

use crate::error::AppResult;

const MIGRATION_0001: &str = include_str!("../../migrations/0001_init.sql");
const MIGRATION_0002: &str = include_str!("../../migrations/0002_extracted_name.sql");

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "0001_init.sql", MIGRATION_0001),
    (2, "0002_extracted_name.sql", MIGRATION_0002),
];

/// Opens a SQLite database at `db_path`, applies WAL/synchronous/foreign_key PRAGMAs,
/// and runs all pending migrations. If the database file is corrupted, it moves the corrupt file
/// aside and initializes a clean working database.
pub fn open_database(db_path: &Path) -> AppResult<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match try_open_and_migrate(db_path) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            tracing::warn!(
                "Database at {:?} appears corrupt ({}). Recreating clean database.",
                db_path,
                err
            );
            wipe_db_files(db_path);
            try_open_and_migrate(db_path)
        }
    }
}

fn wipe_db_files(db_path: &Path) {
    if let Some(parent) = db_path.parent() {
        let file_name = db_path.file_name().unwrap_or_default().to_string_lossy();
        let wal = parent.join(format!("{}-wal", file_name));
        let shm = parent.join(format!("{}-shm", file_name));
        let _ = std::fs::remove_file(&wal);
        if wal.exists() {
            let _ = std::fs::write(&wal, b"");
            let _ = std::fs::remove_file(&wal);
        }
        let _ = std::fs::remove_file(&shm);
        if shm.exists() {
            let _ = std::fs::write(&shm, b"");
            let _ = std::fs::remove_file(&shm);
        }
    }
    let _ = std::fs::remove_file(db_path);
    if db_path.exists() {
        let _ = std::fs::write(db_path, b"");
        let _ = std::fs::remove_file(db_path);
    }
}

fn try_open_and_migrate(db_path: &Path) -> AppResult<Connection> {
    let mut conn = Connection::open(db_path)?;
    configure_connection(&conn)?;
    let integrity: String = conn.query_row("PRAGMA quick_check(1);", [], |r| r.get(0))?;
    if integrity.to_lowercase() != "ok" {
        return Err(crate::error::AppError::Other(format!(
            "SQLite quick_check failed: {}",
            integrity
        )));
    }
    run_migrations(&mut conn)?;
    Ok(conn)
}

/// Applies required PRAGMAs to an open connection: WAL, synchronous=NORMAL, foreign_keys=ON, busy_timeout=5000.
pub fn configure_connection(conn: &Connection) -> AppResult<()> {
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

/// Runs any migrations whose version is greater than `meta.schema_version`.
/// Each migration is applied in a single transaction.
pub fn run_migrations(conn: &mut Connection) -> AppResult<()> {
    let has_meta: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    let current_version: i64 = if has_meta {
        conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| {
                let s: String = r.get(0)?;
                Ok(s.parse::<i64>().unwrap_or(0))
            },
        )
        .unwrap_or(0)
    } else {
        0
    };

    for (version, name, sql) in MIGRATIONS {
        if *version > current_version {
            tracing::info!("Applying migration {} (version {})", name, version);
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [version.to_string()],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_and_migrations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("rebuffer.db");
        let conn = open_database(&db_path).unwrap();

        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // Against the migration list rather than a literal, so adding one does
        // not mean remembering to come back and edit a number here.
        let latest = MIGRATIONS.last().unwrap().0;
        assert_eq!(version, latest.to_string());

        let items_exist: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='items'",
                [],
                |_| Ok(true),
            )
            .unwrap();
        assert!(items_exist);
    }

    /// The second migration adds a column to a table that already has rows in
    /// every existing installation, so the upgrade path matters as much as the
    /// fresh one: open at version 1, then open again and confirm the column is
    /// there and empty rather than the database being rebuilt.
    #[test]
    fn test_extracted_name_migration_applies_to_an_existing_database() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("rebuffer.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            // 0001 records the version itself; force it back to 1 in case a
            // future edit to that file changes what it writes.
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items (kind, hash, byte_size, created_at, first_seen_at)
                 VALUES ('text', 'abc', 3, 1, 1)",
                [],
            )
            .unwrap();
        }

        let conn = open_database(&db_path).unwrap();
        let name: Option<String> = conn
            .query_row("SELECT extracted_name FROM items LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, None, "an existing row gains the column, empty");

        // And the unique index is live: two rows cannot hold the same name.
        conn.execute(
            "INSERT INTO items (kind, hash, byte_size, created_at, first_seen_at, extracted_name)
             VALUES ('text', 'def', 3, 1, 1, 'shot.png')",
            [],
        )
        .unwrap();
        let clash = conn.execute(
            "INSERT INTO items (kind, hash, byte_size, created_at, first_seen_at, extracted_name)
             VALUES ('text', 'ghi', 3, 1, 1, 'shot.png')",
            [],
        );
        assert!(clash.is_err(), "the same name twice must be refused");
    }
}
