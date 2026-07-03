use anyhow::Result;
use rusqlite::{params, Connection, Row};

#[derive(Clone)]
pub struct StatRow {
    pub repo: String,
    pub valid_from_sha: String,
    pub valid_from_date: String,
    pub valid_to_sha: Option<String>,
    pub valid_to_date: Option<String>,
    pub row_type: String, // "file" | "folder" | "repo"
    pub folder: String,
    pub folder_depth: i32,
    pub file_name: Option<String>,

    pub file_count: i64,
    pub sloc_nonblank: i64,
    pub sloc_noncomment: i64,

    pub file_type: Option<String>,
    pub source_file_count: i64,
    pub test_file_count: i64,
    pub story_file_count: i64,
    pub config_file_count: i64,

    pub js_exports_default: Option<i64>,
    pub js_exports_named: Option<i64>,
    pub js_exports_total: Option<i64>,
    pub js_export_matches_filename: i64,

    pub py_file_count: i64,
    pub js_file_count: i64,
    pub jsx_file_count: i64,
    pub ts_file_count: i64,
    pub tsx_file_count: i64,
    pub css_file_count: i64,
    pub html_file_count: i64,
    pub md_file_count: i64,
    pub json_file_count: i64,
    pub yaml_file_count: i64,
}

const SELECT_COLS: &str = "repo, valid_from_sha, valid_from_date, valid_to_sha, valid_to_date,
     row_type, folder, folder_depth, file_name,
     file_count, sloc_nonblank, sloc_noncomment,
     file_type, source_file_count, test_file_count, story_file_count, config_file_count,
     js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename,
     py_file_count, js_file_count, jsx_file_count, ts_file_count, tsx_file_count,
     css_file_count, html_file_count, md_file_count, json_file_count, yaml_file_count";

fn row_from_sql(row: &Row<'_>) -> rusqlite::Result<StatRow> {
    Ok(StatRow {
        repo: row.get(0)?,
        valid_from_sha: row.get(1)?,
        valid_from_date: row.get(2)?,
        valid_to_sha: row.get(3)?,
        valid_to_date: row.get(4)?,
        row_type: row.get(5)?,
        folder: row.get(6)?,
        folder_depth: row.get(7)?,
        file_name: row.get(8)?,
        file_count: row.get(9)?,
        sloc_nonblank: row.get(10)?,
        sloc_noncomment: row.get(11)?,
        file_type: row.get(12)?,
        source_file_count: row.get(13)?,
        test_file_count: row.get(14)?,
        story_file_count: row.get(15)?,
        config_file_count: row.get(16)?,
        js_exports_default: row.get(17)?,
        js_exports_named: row.get(18)?,
        js_exports_total: row.get(19)?,
        js_export_matches_filename: row.get(20)?,
        py_file_count: row.get(21)?,
        js_file_count: row.get(22)?,
        jsx_file_count: row.get(23)?,
        ts_file_count: row.get(24)?,
        tsx_file_count: row.get(25)?,
        css_file_count: row.get(26)?,
        html_file_count: row.get(27)?,
        md_file_count: row.get(28)?,
        json_file_count: row.get(29)?,
        yaml_file_count: row.get(30)?,
    })
}

/// Returns true if a repo-wide aggregate row already exists with `valid_from_sha` equal
/// to `commit_sha`. Used to skip re-analysis of already-processed commits.
pub fn commit_exists(conn: &Connection, repo: &str, commit_sha: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM stats
         WHERE repo = ?1 AND valid_from_sha = ?2 AND row_type = 'repo'",
        params![repo, commit_sha],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Returns the repo-level row matching a specific commit_sha as valid_from_sha.
#[allow(dead_code)]
pub fn get_repo_row(conn: &Connection, repo: &str, commit_sha: &str) -> Result<Option<StatRow>> {
    let sql = format!(
        "SELECT {} FROM stats WHERE repo = ?1 AND valid_from_sha = ?2 AND row_type = 'repo'",
        SELECT_COLS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![repo, commit_sha])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_from_sql(row)?)),
        None => Ok(None),
    }
}

/// Returns the currently-open repo-level row (valid_to_sha IS NULL) for the given repo.
pub fn get_current_repo_row(conn: &Connection, repo: &str) -> Result<Option<StatRow>> {
    let sql = format!(
        "SELECT {} FROM stats WHERE repo = ?1 AND row_type = 'repo' AND valid_to_sha IS NULL",
        SELECT_COLS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![repo])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_from_sql(row)?)),
        None => Ok(None),
    }
}

#[allow(dead_code)]
pub fn get_all_file_rows(conn: &Connection, repo: &str, commit_date: &str) -> Result<Vec<StatRow>> {
    let sql = format!(
        "SELECT {} FROM stats
         WHERE repo = ?1 AND row_type = 'file'
           AND valid_from_date <= ?2
           AND (valid_to_date IS NULL OR valid_to_date > ?2)",
        SELECT_COLS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![repo, commit_date], row_from_sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Returns the currently-open folder rows (valid_to_sha IS NULL) for the given folders.
pub fn get_folder_rows_for_folders(
    conn: &Connection,
    repo: &str,
    folders: &[String],
) -> Result<Vec<StatRow>> {
    if folders.is_empty() {
        return Ok(vec![]);
    }
    let placeholders = (2..2 + folders.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM stats
         WHERE repo = ?1 AND row_type = 'folder' AND valid_to_sha IS NULL
           AND folder IN ({})",
        SELECT_COLS, placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut all_params: Vec<rusqlite::types::Value> = vec![repo.to_string().into()];
    for f in folders {
        all_params.push(f.clone().into());
    }
    let rows = stmt.query_map(rusqlite::params_from_iter(all_params), row_from_sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Closes (sets valid_to) on all currently-open rows for the given file paths.
/// Used before inserting updated rows for changed files.
pub fn close_rows_for_file_paths(
    conn: &Connection,
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
    paths: &[String],
) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let placeholders = (4..4 + paths.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE stats SET valid_to_sha = ?2, valid_to_date = ?3
         WHERE repo = ?1 AND row_type = 'file' AND valid_to_sha IS NULL
           AND file_name IN ({})",
        placeholders
    );
    let mut all_params: Vec<rusqlite::types::Value> = vec![
        repo.to_string().into(),
        commit_sha.to_string().into(),
        commit_date.to_string().into(),
    ];
    for p in paths {
        all_params.push(p.clone().into());
    }
    conn.execute(&sql, rusqlite::params_from_iter(all_params))?;
    Ok(())
}

/// Closes (sets valid_to) on all currently-open folder rows for the given folders.
pub fn close_rows_for_folders(
    conn: &Connection,
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
    folders: &[String],
) -> Result<()> {
    if folders.is_empty() {
        return Ok(());
    }
    let placeholders = (4..4 + folders.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE stats SET valid_to_sha = ?2, valid_to_date = ?3
         WHERE repo = ?1 AND row_type = 'folder' AND valid_to_sha IS NULL
           AND folder IN ({})",
        placeholders
    );
    let mut all_params: Vec<rusqlite::types::Value> = vec![
        repo.to_string().into(),
        commit_sha.to_string().into(),
        commit_date.to_string().into(),
    ];
    for f in folders {
        all_params.push(f.clone().into());
    }
    conn.execute(&sql, rusqlite::params_from_iter(all_params))?;
    Ok(())
}

/// Closes (sets valid_to) on the currently-open repo-level row.
pub fn close_repo_row(
    conn: &Connection,
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE stats SET valid_to_sha = ?2, valid_to_date = ?3
         WHERE repo = ?1 AND row_type = 'repo' AND valid_to_sha IS NULL",
        params![repo, commit_sha, commit_date],
    )?;
    Ok(())
}

/// Closes ALL currently-open rows for a repo (all row types).
/// Used before a full-tree re-analysis.
pub fn close_all_open_rows(
    conn: &Connection,
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE stats SET valid_to_sha = ?2, valid_to_date = ?3
         WHERE repo = ?1 AND valid_to_sha IS NULL",
        params![repo, commit_sha, commit_date],
    )?;
    Ok(())
}

pub fn committed_shas(conn: &Connection, repo: &str) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT valid_from_sha FROM stats WHERE repo = ?1 AND row_type = 'repo'",
    )?;
    let rows = stmt.query_map(params![repo], |row| row.get::<_, String>(0))?;
    let mut set = std::collections::HashSet::new();
    for r in rows {
        set.insert(r?);
    }
    Ok(set)
}

pub fn insert_rows(conn: &Connection, rows: &[StatRow]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO stats
            (repo, valid_from_sha, valid_from_date, valid_to_sha, valid_to_date,
             row_type, folder, folder_depth, file_name,
             file_count, sloc_nonblank, sloc_noncomment,
             file_type, source_file_count, test_file_count, story_file_count, config_file_count,
             js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename,
             py_file_count, js_file_count, jsx_file_count, ts_file_count, tsx_file_count,
             css_file_count, html_file_count, md_file_count, json_file_count, yaml_file_count)
         VALUES
            (?1, ?2, ?3, ?4, ?5,
             ?6, ?7, ?8, ?9,
             ?10, ?11, ?12,
             ?13, ?14, ?15, ?16, ?17,
             ?18, ?19, ?20, ?21,
             ?22, ?23, ?24, ?25, ?26,
             ?27, ?28, ?29, ?30, ?31)",
    )?;

    for row in rows {
        stmt.execute(params![
            row.repo,
            row.valid_from_sha,
            row.valid_from_date,
            row.valid_to_sha,
            row.valid_to_date,
            row.row_type,
            row.folder,
            row.folder_depth,
            row.file_name,
            row.file_count,
            row.sloc_nonblank,
            row.sloc_noncomment,
            row.file_type,
            row.source_file_count,
            row.test_file_count,
            row.story_file_count,
            row.config_file_count,
            row.js_exports_default,
            row.js_exports_named,
            row.js_exports_total,
            row.js_export_matches_filename,
            row.py_file_count,
            row.js_file_count,
            row.jsx_file_count,
            row.ts_file_count,
            row.tsx_file_count,
            row.css_file_count,
            row.html_file_count,
            row.md_file_count,
            row.json_file_count,
            row.yaml_file_count,
        ])?;
    }

    Ok(())
}
