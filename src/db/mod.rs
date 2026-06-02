pub mod schema;
pub mod store;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         -- 128 MB page cache (negative value = KiB)
         PRAGMA cache_size = -131072;
         -- Keep temp tables in memory instead of a temp file
         PRAGMA temp_store = MEMORY;
         -- 512 MB memory-mapped I/O for read-heavy query sessions
         PRAGMA mmap_size = 536870912;
        ",
    )?;
    Ok(conn)
}
