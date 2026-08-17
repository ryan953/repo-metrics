use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

pub struct CommitRow {
    pub repo: String,
    pub sha: String,
    pub author_date: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_date: String,
    pub committer_name: String,
    pub committer_email: String,
    /// Diff size against the first parent. `None` when the caller skipped computing it
    /// this run (see `needs_line_stats`) — an existing row's stats are left untouched
    /// rather than clobbered with NULLs.
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub files_changed: Option<i64>,
}

/// Returns true if `(repo, sha)` has no row yet, or has a row whose line stats haven't
/// been computed yet (pre-migration row, or backfill in progress). Callers use this to
/// decide whether it's worth paying for a diff via `git::commit_line_stats` before
/// calling `insert_commit` — once a commit's stats are recorded, this returns false
/// forever, so re-running `analyze` never re-diffs an already-backfilled commit.
pub fn needs_line_stats(conn: &Connection, repo: &str, sha: &str) -> Result<bool> {
    let additions: Option<Option<i64>> = conn
        .query_row(
            "SELECT additions FROM commits WHERE repo = ?1 AND sha = ?2",
            params![repo, sha],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match additions {
        None => true,           // no row yet
        Some(None) => true,     // row exists, stats not yet computed
        Some(Some(_)) => false, // already backfilled
    })
}

/// Inserts a commit row, or — if `(repo, sha)` already exists — backfills its line
/// stats when `row` carries some and the existing row doesn't yet. Identity/date fields
/// on an existing row are never touched (they can't change: SHAs are content-addressed).
///
/// Idempotent and safe to call for every commit on every run, including commits whose
/// `stats` rows were already recorded on a previous run: the identity-only path (no line
/// stats computed) is cheap, and the conflict branch is a no-op once stats are recorded.
pub fn insert_commit(conn: &Connection, row: &CommitRow) -> Result<()> {
    conn.execute(
        "INSERT INTO commits
            (repo, sha, author_date, author_name, author_email,
             committer_date, committer_name, committer_email,
             additions, deletions, files_changed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(repo, sha) DO UPDATE SET
             additions = excluded.additions,
             deletions = excluded.deletions,
             files_changed = excluded.files_changed
         WHERE commits.additions IS NULL AND excluded.additions IS NOT NULL",
        params![
            row.repo,
            row.sha,
            row.author_date,
            row.author_name,
            row.author_email,
            row.committer_date,
            row.committer_name,
            row.committer_email,
            row.additions,
            row.deletions,
            row.files_changed,
        ],
    )?;
    Ok(())
}
