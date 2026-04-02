use anyhow::Result;
use rusqlite::{params, Connection, Row};

#[derive(Clone)]
pub struct StatRow {
    pub repo: String,
    pub commit_sha: String,
    pub commit_date: String,
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

const SELECT_COLS: &str =
    "repo, commit_sha, commit_date, row_type, folder, folder_depth, file_name,
     file_count, sloc_nonblank, sloc_noncomment,
     file_type, source_file_count, test_file_count, story_file_count, config_file_count,
     js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename,
     py_file_count, js_file_count, jsx_file_count, ts_file_count, tsx_file_count,
     css_file_count, html_file_count, md_file_count, json_file_count, yaml_file_count";

fn row_from_sql(row: &Row<'_>) -> rusqlite::Result<StatRow> {
    Ok(StatRow {
        repo: row.get(0)?,
        commit_sha: row.get(1)?,
        commit_date: row.get(2)?,
        row_type: row.get(3)?,
        folder: row.get(4)?,
        folder_depth: row.get(5)?,
        file_name: row.get(6)?,
        file_count: row.get(7)?,
        sloc_nonblank: row.get(8)?,
        sloc_noncomment: row.get(9)?,
        file_type: row.get(10)?,
        source_file_count: row.get(11)?,
        test_file_count: row.get(12)?,
        story_file_count: row.get(13)?,
        config_file_count: row.get(14)?,
        js_exports_default: row.get(15)?,
        js_exports_named: row.get(16)?,
        js_exports_total: row.get(17)?,
        js_export_matches_filename: row.get(18)?,
        py_file_count: row.get(19)?,
        js_file_count: row.get(20)?,
        jsx_file_count: row.get(21)?,
        ts_file_count: row.get(22)?,
        tsx_file_count: row.get(23)?,
        css_file_count: row.get(24)?,
        html_file_count: row.get(25)?,
        md_file_count: row.get(26)?,
        json_file_count: row.get(27)?,
        yaml_file_count: row.get(28)?,
    })
}

/// Returns true if a repo-wide aggregate row already exists for this (repo, commit_sha).
/// Used to skip re-analysis of already-processed commits.
pub fn commit_exists(conn: &Connection, repo: &str, commit_sha: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM stats
         WHERE repo = ?1 AND commit_sha = ?2 AND row_type = 'repo'",
        params![repo, commit_sha],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn get_repo_row(conn: &Connection, repo: &str, commit_sha: &str) -> Result<Option<StatRow>> {
    let sql = format!(
        "SELECT {} FROM stats WHERE repo = ?1 AND commit_sha = ?2 AND row_type = 'repo'",
        SELECT_COLS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![repo, commit_sha])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_from_sql(row)?)),
        None => Ok(None),
    }
}

#[allow(dead_code)]
pub fn get_all_file_rows(conn: &Connection, repo: &str, commit_sha: &str) -> Result<Vec<StatRow>> {
    let sql = format!(
        "SELECT {} FROM stats WHERE repo = ?1 AND commit_sha = ?2 AND row_type = 'file'",
        SELECT_COLS
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![repo, commit_sha], row_from_sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn get_folder_rows_for_folders(
    conn: &Connection,
    repo: &str,
    commit_sha: &str,
    folders: &[String],
) -> Result<Vec<StatRow>> {
    if folders.is_empty() {
        return Ok(vec![]);
    }
    let placeholders = (3..3 + folders.len())
        .map(|i| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM stats WHERE repo = ?1 AND commit_sha = ?2 AND row_type = 'folder' AND folder IN ({})",
        SELECT_COLS, placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut all_params: Vec<rusqlite::types::Value> = vec![
        repo.to_string().into(),
        commit_sha.to_string().into(),
    ];
    for f in folders {
        all_params.push(f.clone().into());
    }
    let rows = stmt.query_map(rusqlite::params_from_iter(all_params), row_from_sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Copies file rows from `from_sha` to `to_sha`, excluding any paths in `exclude_paths`.
pub fn copy_file_rows_from_parent(
    conn: &Connection,
    repo: &str,
    from_sha: &str,
    to_sha: &str,
    to_date: &str,
    exclude_paths: &[String],
) -> Result<()> {
    let not_in_clause = if exclude_paths.is_empty() {
        String::new()
    } else {
        let placeholders = (5..5 + exclude_paths.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" AND COALESCE(file_name, '') NOT IN ({})", placeholders)
    };
    let sql = format!(
        "INSERT INTO stats ({cols})
         SELECT repo, ?2, ?3, row_type, folder, folder_depth, file_name,
                file_count, sloc_nonblank, sloc_noncomment,
                file_type, source_file_count, test_file_count, story_file_count, config_file_count,
                js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename
         FROM stats
         WHERE repo = ?1 AND commit_sha = ?4 AND row_type = 'file'{not_in}",
        cols = SELECT_COLS,
        not_in = not_in_clause,
    );
    let mut all_params: Vec<rusqlite::types::Value> = vec![
        repo.to_string().into(),
        to_sha.to_string().into(),
        to_date.to_string().into(),
        from_sha.to_string().into(),
    ];
    for p in exclude_paths {
        all_params.push(p.clone().into());
    }
    conn.execute(&sql, rusqlite::params_from_iter(all_params))?;
    Ok(())
}

/// Copies folder rows from `from_sha` to `to_sha`, excluding any folders in `exclude_folders`.
pub fn copy_folder_rows_from_parent(
    conn: &Connection,
    repo: &str,
    from_sha: &str,
    to_sha: &str,
    to_date: &str,
    exclude_folders: &[String],
) -> Result<()> {
    let not_in_clause = if exclude_folders.is_empty() {
        String::new()
    } else {
        let placeholders = (5..5 + exclude_folders.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" AND folder NOT IN ({})", placeholders)
    };
    let sql = format!(
        "INSERT INTO stats ({cols})
         SELECT repo, ?2, ?3, row_type, folder, folder_depth, file_name,
                file_count, sloc_nonblank, sloc_noncomment,
                file_type, source_file_count, test_file_count, story_file_count, config_file_count,
                js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename
         FROM stats
         WHERE repo = ?1 AND commit_sha = ?4 AND row_type = 'folder'{not_in}",
        cols = SELECT_COLS,
        not_in = not_in_clause,
    );
    let mut all_params: Vec<rusqlite::types::Value> = vec![
        repo.to_string().into(),
        to_sha.to_string().into(),
        to_date.to_string().into(),
        from_sha.to_string().into(),
    ];
    for f in exclude_folders {
        all_params.push(f.clone().into());
    }
    conn.execute(&sql, rusqlite::params_from_iter(all_params))?;
    Ok(())
}

pub fn insert_rows(conn: &Connection, rows: &[StatRow]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO stats
            (repo, commit_sha, commit_date, row_type, folder, folder_depth, file_name,
             file_count, sloc_nonblank, sloc_noncomment,
             file_type, source_file_count, test_file_count, story_file_count, config_file_count,
             js_exports_default, js_exports_named, js_exports_total, js_export_matches_filename,
             py_file_count, js_file_count, jsx_file_count, ts_file_count, tsx_file_count,
             css_file_count, html_file_count, md_file_count, json_file_count, yaml_file_count)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7,
             ?8, ?9, ?10,
             ?11, ?12, ?13, ?14, ?15,
             ?16, ?17, ?18, ?19,
             ?20, ?21, ?22, ?23, ?24,
             ?25, ?26, ?27, ?28, ?29)",
    )?;

    for row in rows {
        stmt.execute(params![
            row.repo,
            row.commit_sha,
            row.commit_date,
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
