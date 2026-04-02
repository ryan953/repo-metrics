# repo-metrics

A CLI tool that walks a git repository's full commit history and collects per-file, per-folder, and repo-wide statistics into a SQLite database for analysis over time.

## Building

Requires Rust (install via [rustup](https://rustup.rs)) and a C compiler. SQLite is bundled — no system dependency needed.

```bash
# Development build
cargo build

# Optimized build (recommended for large repos)
cargo build --release
```

The binary is written to `target/debug/repo-metrics` or `target/release/repo-metrics`.

## Usage

```
repo-metrics analyze <REPO_PATH> --repo <ORG/REPO> [OPTIONS]
```

### Arguments

| Argument      | Description                        |
|---------------|------------------------------------|
| `<REPO_PATH>` | Path to the git repository on disk |

### Options

| Option                 | Default           | Description                                                                                                                                   |
|------------------------|-------------------|-----------------------------------------------------------------------------------------------------------------------------------------------|
| `--repo <ORG/REPO>`    | *(required)*      | Repository identity in `org/repo` format (e.g. `getsentry/sentry`). Used as the primary key in the database — must be consistent across runs. |
| `--commit <REF>`       | `HEAD`            | Tip of the commit walk. All ancestors of this ref are analyzed. Accepts a branch name, tag, or full SHA.                                      |
| `--db <PATH>`          | `repo-metrics.db` | Path to the SQLite database file. Created if it doesn't exist.                                                                                |
| `--since <YYYY-MM-DD>` | *(none)*          | Oldest commit date to include. Commits before this date are excluded. Omit to walk the full history.                                          |
| `--granularity <LEVEL>` | `all`            | How much detail to store per commit: `all` (file + folder + repo rows), `folder` (folder + repo rows only), or `repo` (one row per commit). File-level analysis always runs; this only controls what gets written to the database. |

### Examples

Analyze the full history of a local repo:
```bash
repo-metrics analyze ~/code/sentry --repo getsentry/sentry
```

Write to a specific database file:
```bash
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry.db
```

Analyze only the history reachable from a specific branch or tag:
```bash
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --commit origin/main
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --commit v24.1.0
```

Run incrementally — already-analyzed commits are skipped automatically:
```bash
# First run: processes all commits
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry.db

# After pulling new commits: only processes the new ones
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry.db
```

Store only repo-level rows (smallest DB, fastest for trend analysis):
```bash
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry-repo.db --granularity repo
```

> **Note:** granularity is a one-way commitment per database. Once a commit is recorded at a given granularity, re-running with a finer granularity will skip those commits. Use a separate `--db` file if you need a different level of detail.

Limit history to the last few years:
```bash
repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry.db --since 2022-01-01
```

Run in the background for large repos:
```bash
nohup repo-metrics analyze ~/code/sentry --repo getsentry/sentry --db sentry.db \
  > metrics.log 2>&1 &
tail -f metrics.log
```

## Database schema

All stats are written to a single `stats` table in the SQLite database. Each analyzed commit produces three tiers of rows:

| `row_type` | `folder`         | `file_name`                 | Represents                                      |
|------------|------------------|-----------------------------|-------------------------------------------------|
| `file`     | `src/components` | `src/components/Button.tsx` | One file                                        |
| `folder`   | `src/components` | *(NULL)*                    | Aggregate for all files directly in that folder |
| `repo`     | `.`              | *(NULL)*                    | Aggregate for the entire repo                   |

### Columns

| Column                       | Type | Description                                                                                                            |
|------------------------------|------|------------------------------------------------------------------------------------------------------------------------|
| `repo`                       | TEXT | Repository identity (`org/repo`)                                                                                       |
| `commit_sha`                 | TEXT | Full 40-character commit SHA                                                                                           |
| `commit_date`                | TEXT | Commit timestamp in ISO 8601 format                                                                                    |
| `row_type`                   | TEXT | `file`, `folder`, or `repo`                                                                                            |
| `folder`                     | TEXT | Folder path relative to repo root; `.` for root-level files and the repo aggregate                                     |
| `folder_depth`               | INT  | Number of path components in `folder`; `0` for `.`, `1` for `src`, etc.                                                |
| `file_name`                  | TEXT | File path relative to repo root; NULL on `folder` and `repo` rows                                                      |
| `file_count`                 | INT  | `1` on file rows; count of files on aggregate rows                                                                     |
| `sloc_nonblank`              | INT  | Lines that are not empty or whitespace-only                                                                            |
| `sloc_noncomment`            | INT  | Non-blank lines that are also not comment lines                                                                        |
| `file_type`                  | TEXT | JS/TS only: `source`, `test`, `story`, or `config`. NULL for all other files and aggregate rows.                       |
| `source_file_count`          | INT  | `1` if `file_type = 'source'`, else `0`; pre-summed on aggregate rows                                                  |
| `test_file_count`            | INT  | `1` if `file_type = 'test'`, else `0`; pre-summed on aggregate rows                                                    |
| `story_file_count`           | INT  | `1` if `file_type = 'story'`, else `0`; pre-summed on aggregate rows                                                   |
| `config_file_count`          | INT  | `1` if `file_type = 'config'`, else `0`; pre-summed on aggregate rows                                                  |
| `js_exports_default`         | INT  | Count of `export default` statements; NULL for non-JS/TS files                                                         |
| `js_exports_named`           | INT  | Count of named exports (declarations + list entries); NULL for non-JS/TS files                                         |
| `js_exports_total`           | INT  | `js_exports_default + js_exports_named`; NULL for non-JS/TS files                                                      |
| `js_export_matches_filename` | INT  | `1` if any export's public name matches the file stem (case-insensitive); `0` otherwise. Pre-summed on aggregate rows. |

### File type classification (JS/TS only)

| `file_type` | Matched by                                                                                          |
|-------------|-----------------------------------------------------------------------------------------------------|
| `test`      | `*.test.*`, `*.spec.*`, files under `__tests__/`, `tests/`, or `test/` directories                  |
| `story`     | `*.stories.*`, `*.story.*`                                                                          |
| `config`    | `*.config.*`, known config filenames (`jest.config.ts`, `vite.config.ts`, `eslint.config.js`, etc.) |
| `source`    | All other JS/TS files                                                                               |

### Example queries

```sql
-- SLOC growth over time for the whole repo
SELECT commit_date, file_count, sloc_nonblank, sloc_noncomment
FROM stats
WHERE repo = 'getsentry/sentry' AND row_type = 'repo'
ORDER BY commit_date;

-- Test ratio per folder at a specific commit
SELECT folder,
       source_file_count,
       test_file_count,
       ROUND(test_file_count * 100.0 / NULLIF(source_file_count + test_file_count, 0), 1) AS test_pct
FROM stats
WHERE repo = 'getsentry/sentry'
  AND commit_sha = 'abc123...'
  AND row_type = 'folder'
ORDER BY test_pct ASC;

-- Files where no export matches the filename (missing primary export)
SELECT file_name, js_exports_total
FROM stats
WHERE repo = 'getsentry/sentry'
  AND commit_sha = 'abc123...'
  AND row_type = 'file'
  AND file_type = 'source'
  AND js_export_matches_filename = 0
ORDER BY js_exports_total DESC;

-- Story count growth over time
SELECT commit_date, story_file_count
FROM stats
WHERE repo = 'getsentry/sentry' AND row_type = 'repo'
ORDER BY commit_date;
```
