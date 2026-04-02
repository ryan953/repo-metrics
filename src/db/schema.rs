use anyhow::Result;
use rusqlite::Connection;

pub fn ensure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS stats (
            id              INTEGER PRIMARY KEY,
            repo            TEXT    NOT NULL,
            commit_sha      TEXT    NOT NULL,
            commit_date     TEXT    NOT NULL,

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

        -- Lightweight index kept at all times for fast commit_exists lookups.
        CREATE INDEX IF NOT EXISTS idx_commit_lookup
            ON stats(repo, commit_sha, row_type);
        ",
    )?;
    // Unique index is managed separately so it can be deferred during bulk loads.
    ensure_unique_index(conn)?;
    Ok(())
}

/// Full uniqueness index. Expensive to maintain during bulk inserts — drop it with
/// `drop_unique_index` before a bulk run and call this again when done.
pub fn ensure_unique_index(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_stats_unique
             ON stats(repo, commit_sha, row_type, folder, COALESCE(file_name, ''));",
    )?;
    Ok(())
}

pub fn drop_unique_index(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_stats_unique;")?;
    Ok(())
}
