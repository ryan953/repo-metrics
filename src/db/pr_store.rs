use anyhow::Result;
use rusqlite::{params, Connection};

pub struct PullRequestRow {
    pub repo: String,
    pub pr_number: i64,
    pub title: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub closed_at: Option<String>,
    pub merged: bool,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_files: Option<i64>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
}

pub struct ReviewRow {
    pub repo: String,
    pub pr_number: i64,
    pub review_id: i64,
    pub reviewer: String,
    pub state: String,
    pub submitted_at: String,
}

pub struct RequestedReviewerRow {
    pub repo: String,
    pub pr_number: i64,
    pub reviewer: String,
    pub reviewer_type: String,
}

pub fn upsert_pull_request(conn: &Connection, row: &PullRequestRow) -> Result<()> {
    conn.execute(
        "INSERT INTO pull_requests
            (repo, pr_number, title, author, state, draft, created_at, updated_at,
             merged_at, closed_at, merged, additions, deletions, changed_files,
             base_ref, head_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(repo, pr_number) DO UPDATE SET
            title = excluded.title,
            author = excluded.author,
            state = excluded.state,
            draft = excluded.draft,
            updated_at = excluded.updated_at,
            merged_at = excluded.merged_at,
            closed_at = excluded.closed_at,
            merged = excluded.merged,
            additions = excluded.additions,
            deletions = excluded.deletions,
            changed_files = excluded.changed_files,
            base_ref = excluded.base_ref,
            head_ref = excluded.head_ref",
        params![
            row.repo,
            row.pr_number,
            row.title,
            row.author,
            row.state,
            row.draft,
            row.created_at,
            row.updated_at,
            row.merged_at,
            row.closed_at,
            row.merged,
            row.additions,
            row.deletions,
            row.changed_files,
            row.base_ref,
            row.head_ref,
        ],
    )?;
    Ok(())
}

pub fn upsert_review(conn: &Connection, row: &ReviewRow) -> Result<()> {
    conn.execute(
        "INSERT INTO pr_reviews
            (repo, pr_number, review_id, reviewer, state, submitted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(repo, pr_number, review_id) DO UPDATE SET
            reviewer = excluded.reviewer,
            state = excluded.state,
            submitted_at = excluded.submitted_at",
        params![
            row.repo,
            row.pr_number,
            row.review_id,
            row.reviewer,
            row.state,
            row.submitted_at,
        ],
    )?;
    Ok(())
}

pub fn replace_requested_reviewers(
    conn: &Connection,
    repo: &str,
    pr_number: i64,
    rows: &[RequestedReviewerRow],
) -> Result<()> {
    conn.execute(
        "DELETE FROM pr_reviewers_requested WHERE repo = ?1 AND pr_number = ?2",
        params![repo, pr_number],
    )?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO pr_reviewers_requested (repo, pr_number, reviewer, reviewer_type)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for row in rows {
        stmt.execute(params![row.repo, row.pr_number, row.reviewer, row.reviewer_type])?;
    }
    Ok(())
}

pub fn pr_updated_at(
    conn: &Connection,
    repo: &str,
    pr_number: i64,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT updated_at FROM pull_requests WHERE repo = ?1 AND pr_number = ?2",
    )?;
    let mut rows = stmt.query(params![repo, pr_number])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}
