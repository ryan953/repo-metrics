use anyhow::Result;
use rusqlite::Connection;

pub fn ensure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS stats (
            id              INTEGER PRIMARY KEY,
            repo            TEXT    NOT NULL,

            -- Validity range (SCD Type 2): the commit range where this row is current
            valid_from_sha  TEXT    NOT NULL,
            valid_from_date TEXT    NOT NULL,
            valid_to_sha    TEXT,
            valid_to_date   TEXT,

            -- Row type: 'file' | 'folder' | 'repo'
            row_type        TEXT    NOT NULL DEFAULT 'file',

            -- Hierarchy: '.' = repo root, 'src/foo' = folder, file_name NULL = aggregate row
            folder          TEXT    NOT NULL,
            folder_depth    INTEGER NOT NULL,
            file_name       TEXT,

            -- Universal stats (1 for file rows, SUM for aggregate rows)
            file_count         INTEGER NOT NULL DEFAULT 0,
            sloc_nonblank      INTEGER NOT NULL DEFAULT 0,
            sloc_noncomment    INTEGER NOT NULL DEFAULT 0,

            -- JS/TS classification: 'source'|'test'|'story'|'config'|NULL
            -- NULL on non-JS files and all aggregate rows
            file_type          TEXT,

            -- Pre-computed type counts (0/1 on file rows, SUM on aggregate rows)
            source_file_count  INTEGER NOT NULL DEFAULT 0,
            test_file_count    INTEGER NOT NULL DEFAULT 0,
            story_file_count   INTEGER NOT NULL DEFAULT 0,
            config_file_count  INTEGER NOT NULL DEFAULT 0,

            -- JS/TS export stats (NULL on non-JS files)
            js_exports_default        INTEGER,
            js_exports_named          INTEGER,
            js_exports_total          INTEGER,

            -- 0/1 on file rows (1 = some export name matches the file stem),
            -- SUM on aggregate rows (count of files where it matched)
            js_export_matches_filename INTEGER NOT NULL DEFAULT 0,

            -- Per-extension file counts (0/1 on file rows, SUM on aggregate rows)
            -- js = .js/.mjs/.cjs, ts = .ts/.mts/.cts, css = .css/.scss/.sass/.less,
            -- html = .html/.htm, md = .md/.mdx, yaml = .yaml/.yml
            py_file_count      INTEGER NOT NULL DEFAULT 0,
            js_file_count      INTEGER NOT NULL DEFAULT 0,
            jsx_file_count     INTEGER NOT NULL DEFAULT 0,
            ts_file_count      INTEGER NOT NULL DEFAULT 0,
            tsx_file_count     INTEGER NOT NULL DEFAULT 0,
            css_file_count     INTEGER NOT NULL DEFAULT 0,
            html_file_count    INTEGER NOT NULL DEFAULT 0,
            md_file_count      INTEGER NOT NULL DEFAULT 0,
            json_file_count    INTEGER NOT NULL DEFAULT 0,
            yaml_file_count    INTEGER NOT NULL DEFAULT 0
        );

        -- Index for range queries: find all rows valid at a given date
        CREATE INDEX IF NOT EXISTS idx_stats_range
            ON stats(repo, row_type, valid_from_date);

        -- Index for finding current (open) rows for a file when closing them
        CREATE INDEX IF NOT EXISTS idx_stats_current_file
            ON stats(repo, row_type, file_name, valid_to_date);

        -- Index for commit_exists lookups by valid_from_sha
        CREATE INDEX IF NOT EXISTS idx_commit_lookup
            ON stats(repo, valid_from_sha, row_type);
        ",
    )?;
    ensure_unique_index(conn)?;
    Ok(())
}

/// Full uniqueness index. Expensive to maintain during bulk inserts — drop it with
/// `drop_unique_index` before a bulk run and call this again when done.
pub fn ensure_unique_index(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_stats_unique
             ON stats(repo, valid_from_sha, row_type, folder, COALESCE(file_name, ''));",
    )?;
    Ok(())
}

pub fn drop_unique_index(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_stats_unique;")?;
    Ok(())
}

pub fn ensure_pr_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pull_requests (
            id              INTEGER PRIMARY KEY,
            repo            TEXT    NOT NULL,
            pr_number       INTEGER NOT NULL,
            title           TEXT    NOT NULL,
            author          TEXT    NOT NULL,
            state           TEXT    NOT NULL,
            draft           INTEGER NOT NULL DEFAULT 0,
            created_at      TEXT    NOT NULL,
            updated_at      TEXT    NOT NULL,
            merged_at       TEXT,
            closed_at       TEXT,
            merged          INTEGER NOT NULL DEFAULT 0,
            additions       INTEGER,
            deletions       INTEGER,
            changed_files   INTEGER,
            base_ref        TEXT,
            head_ref        TEXT,
            UNIQUE(repo, pr_number)
        );

        CREATE TABLE IF NOT EXISTS pr_reviews (
            id              INTEGER PRIMARY KEY,
            repo            TEXT    NOT NULL,
            pr_number       INTEGER NOT NULL,
            review_id       INTEGER NOT NULL,
            reviewer        TEXT    NOT NULL,
            state           TEXT    NOT NULL,
            submitted_at    TEXT    NOT NULL,
            UNIQUE(repo, pr_number, review_id)
        );

        CREATE INDEX IF NOT EXISTS idx_pr_repo_author
            ON pull_requests(repo, author);
        CREATE INDEX IF NOT EXISTS idx_pr_repo_created
            ON pull_requests(repo, created_at);
        CREATE INDEX IF NOT EXISTS idx_pr_reviews_repo_reviewer
            ON pr_reviews(repo, reviewer);
        ",
    )?;
    Ok(())
}
