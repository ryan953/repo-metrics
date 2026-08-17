use anyhow::Result;
use rusqlite::{params, Connection};

pub struct CommitRow {
    pub repo: String,
    pub sha: String,
    pub author_date: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_date: String,
    pub committer_name: String,
    pub committer_email: String,
}

/// Inserts a commit row, ignoring the insert if `(repo, sha)` already exists.
/// Cheap and idempotent — safe to call for every commit on every run, including
/// commits whose `stats` rows were already recorded on a previous run.
pub fn insert_commit(conn: &Connection, row: &CommitRow) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO commits
            (repo, sha, author_date, author_name, author_email,
             committer_date, committer_name, committer_email)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.repo,
            row.sha,
            row.author_date,
            row.author_name,
            row.author_email,
            row.committer_date,
            row.committer_name,
            row.committer_email,
        ],
    )?;
    Ok(())
}
