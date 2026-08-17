# repo-metrics

A CLI tool that collects git repository statistics into a SQLite database for analysis over time. Walks commit history for per-file, per-folder, and repo-wide code metrics, and fetches pull request data from the GitHub API to track authors and reviewers.

## Building

Requires Rust (install via [rustup](https://rustup.rs)) and a C compiler. SQLite is bundled — no system dependency needed.

```bash
# Development build
cargo build

# Optimized build (recommended for large repos)
cargo build --release
```

The binary is written to `target/debug/repo-metrics` or `target/release/repo-metrics`.

## Releases

Prebuilt binaries for Linux (`x86_64`) and macOS (`aarch64`) are attached to each
[GitHub Release](https://github.com/ryan953/repo-metrics/releases) as `.tar.gz`
tarballs. Download the one for your platform and extract the `repo-metrics` binary.

Releases are automated with [release-plz](https://release-plz.dev) driven by
[Conventional Commits](https://www.conventionalcommits.org):

1. Merge changes to `main` using conventional commit messages (`feat:`, `fix:`,
   `ref:`, …). The commit types determine the next version — `fix:` bumps the
   patch, `feat:` the minor, and a `!` / `BREAKING CHANGE:` the major.
2. release-plz keeps a **release PR** open that bumps the version in `Cargo.toml`
   and regenerates `CHANGELOG.md` from the commits since the last release.
3. **Merging that release PR** is the only manual step. It creates the git tag
   and GitHub Release, then triggers the build that compiles and attaches the
   platform binaries.

To cut a release by hand instead, push a `v*` tag (or run the **Release**
workflow via workflow dispatch) — the same build attaches binaries to the
release for that tag.

## Commands

- **`analyze`** — Walk git commit history and collect code metrics (SLOC, file types, exports) plus per-commit author/committer identity
- **`prs`** — Fetch pull request authors, reviewers, and review activity from the GitHub API
- **`status`** — Show how many commits from git history are loaded in the database

## `analyze`

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

## `prs`

```
repo-metrics prs --repo <ORG/REPO> --token <TOKEN> [OPTIONS]
```

Fetches pull request data from the GitHub REST API and stores it in the database. For each PR, fetches full details (additions/deletions/changed files), all submitted reviews, and requested reviewers (for open PRs). Prints a summary of top authors, top reviewers, and a review-vs-authored ratio.

### Options

| Option                 | Default           | Description                                                              |
|------------------------|-------------------|--------------------------------------------------------------------------|
| `--repo <ORG/REPO>`    | *(required)*      | Repository identity in `org/repo` format (e.g. `getsentry/sentry`).     |
| `--token <TOKEN>`      | *(required)*      | GitHub personal access token. Also reads `GITHUB_TOKEN` env var.         |
| `--db <PATH>`          | `repo-metrics.db` | Path to the SQLite database file.                                        |
| `--since <YYYY-MM-DD>` | *(none)*          | Only fetch PRs created on or after this date. Omit to fetch all PRs.     |

### Examples

Fetch PRs from the last year:
```bash
repo-metrics prs --repo getsentry/sentry --since 2025-01-01
```

Use an env var for the token:
```bash
export GITHUB_TOKEN=$(gh auth token)
repo-metrics prs --repo getsentry/sentry --since 2025-01-01
```

Re-running is incremental — PRs whose `updated_at` hasn't changed are skipped.

### Rate limits

Each PR requires 2–3 GitHub API calls (detail + reviews + requested reviewers for open PRs). The tool reports remaining rate limit at startup. Use `--since` to control scope for large repos.

## `status`

```
repo-metrics status <REPO_PATH> --repo <ORG/REPO> [OPTIONS]
```

Compares the commit history in a local git repo against what's loaded in the database. Reports total commits, loaded count, missing count, coverage percentage, and date ranges.

### Options

| Option                 | Default           | Description                              |
|------------------------|-------------------|------------------------------------------|
| `--repo <ORG/REPO>`    | *(required)*      | Repository identity in `org/repo` format.|
| `--commit <REF>`       | `HEAD`            | Tip of the commit walk.                  |
| `--db <PATH>`          | `repo-metrics.db` | Path to the SQLite database file.        |
| `--since <YYYY-MM-DD>` | *(none)*          | Oldest commit date to include.           |

### Example

```bash
repo-metrics status ~/code/sentry --repo getsentry/sentry --db sentry.db
```

## Database schema

### `stats` table

Written by the `analyze` command. Each analyzed commit produces three tiers of rows:

| `row_type` | `folder`         | `file_name`                 | Represents                                      |
|------------|------------------|-----------------------------|-------------------------------------------------|
| `file`     | `src/components` | `src/components/Button.tsx` | One file                                        |
| `folder`   | `src/components` | *(NULL)*                    | Aggregate for all files directly in that folder |
| `repo`     | `.`              | *(NULL)*                    | Aggregate for the entire repo                   |

#### Columns

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

#### File type classification (JS/TS only)

| `file_type` | Matched by                                                                                          |
|-------------|-----------------------------------------------------------------------------------------------------|
| `test`      | `*.test.*`, `*.spec.*`, files under `__tests__/`, `tests/`, or `test/` directories                  |
| `story`     | `*.stories.*`, `*.story.*`                                                                          |
| `config`    | `*.config.*`, known config filenames (`jest.config.ts`, `vite.config.ts`, `eslint.config.js`, etc.) |
| `source`    | All other JS/TS files                                                                               |

### `commits` table

Written by the `analyze` command, for every commit visited — independent of
`--granularity` and independent of whether that commit's `stats` rows were already
recorded on a previous run. One row per commit.

| Column            | Type | Description                                                             |
|-------------------|------|---------------------------------------------------------------------------|
| `repo`            | TEXT | Repository identity (`org/repo`)                                          |
| `sha`             | TEXT | Full 40-character commit SHA                                              |
| `author_date`     | TEXT | ISO 8601 author date — when the change was originally authored            |
| `author_name`     | TEXT | Author name, as recorded on the commit                                    |
| `author_email`    | TEXT | Author email, as recorded on the commit                                   |
| `committer_date`  | TEXT | ISO 8601 committer date — when the commit object was created (may differ from `author_date` for rebases, squash-merges, cherry-picks, etc.) |
| `committer_name`  | TEXT | Committer name, as recorded on the commit                                 |
| `committer_email` | TEXT | Committer email, as recorded on the commit                                |
| `additions`       | INT  | Lines added, diffed against the commit's first parent (empty tree for a root commit) — same convention as `git show --stat` |
| `deletions`       | INT  | Lines deleted, same diff convention as `additions`                        |
| `files_changed`   | INT  | Number of files touched by the commit. Git's diff model has no separate "modified lines" count — a changed line is one deletion plus one insertion — so `files_changed` is the closest analog: how many files a commit touched, as opposed to how many lines |

Unique on `(repo, sha)`. Inserts are upserts keyed on `(repo, sha)`: re-running
`analyze` never duplicates rows, and an existing row's `additions`/`deletions`/
`files_changed` are only ever written once — the identity/date fields are immutable
(SHAs are content-addressed) and never rewritten.

**Upgrading an existing database:** if you have a database with `stats` history already
populated by an older version of `repo-metrics` (which had no `commits` table, or had one
without line stats), a normal `analyze` re-run against the same `--commit` ref will
create/migrate the `commits` table and backfill it for the *entire* commit history — not
just new commits. A full, expensive re-analysis (dropping/recreating the database) is
**not** required.

This backfill has two different costs, matching the two kinds of columns above:

- **Author/committer identity and dates** are cheap: capturing them is a single object
  lookup with no file diffing, so it happens for every commit in the walk regardless of
  whether that commit's `stats` rows are skipped as already-recorded.
- **`additions`/`deletions`/`files_changed`** require generating a diff against the
  commit's parent, so they're computed only once per commit: `analyze` checks whether a
  commit's row already has line stats before diffing it, so a database that's already
  fully backfilled pays no extra diffing cost on subsequent runs, and only commits still
  missing line stats (new commits, or old rows from before this feature existed) get
  diffed.

### `pull_requests` table

Written by the `prs` command. One row per PR.

| Column          | Type | Description                                    |
|-----------------|------|------------------------------------------------|
| `repo`          | TEXT | Repository identity (`org/repo`)               |
| `pr_number`     | INT  | Pull request number                            |
| `title`         | TEXT | PR title                                       |
| `author`        | TEXT | GitHub login of the PR author                  |
| `state`         | TEXT | `open` or `closed`                             |
| `draft`         | INT  | `1` if the PR is a draft                       |
| `created_at`    | TEXT | ISO 8601 timestamp                             |
| `updated_at`    | TEXT | ISO 8601 timestamp                             |
| `merged_at`     | TEXT | ISO 8601 timestamp; NULL if not merged         |
| `closed_at`     | TEXT | ISO 8601 timestamp; NULL if still open         |
| `merged`        | INT  | `1` if the PR was merged                       |
| `additions`     | INT  | Lines added                                    |
| `deletions`     | INT  | Lines deleted                                  |
| `changed_files` | INT  | Number of files changed                        |
| `base_ref`      | TEXT | Target branch (e.g. `main`)                    |
| `head_ref`      | TEXT | Source branch                                  |

### `pr_reviews` table

One row per submitted review.

| Column         | Type | Description                                                  |
|----------------|------|--------------------------------------------------------------|
| `repo`         | TEXT | Repository identity                                          |
| `pr_number`    | INT  | Pull request number                                          |
| `review_id`    | INT  | GitHub review ID                                             |
| `reviewer`     | TEXT | GitHub login of the reviewer                                 |
| `state`        | TEXT | `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, or `DISMISSED` |
| `submitted_at` | TEXT | ISO 8601 timestamp                                           |

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

-- Commits per author per month
SELECT author_email,
       strftime('%Y-%m', author_date) AS month,
       COUNT(*) AS commits
FROM commits
WHERE repo = 'getsentry/sentry'
GROUP BY author_email, month
ORDER BY month, commits DESC;

-- Commit size over time: not just how many commits per month, but how big they were
SELECT strftime('%Y-%m', author_date) AS month,
       COUNT(*) AS commits,
       SUM(additions) AS additions,
       SUM(deletions) AS deletions,
       ROUND(AVG(additions + deletions), 1) AS avg_commit_size
FROM commits
WHERE repo = 'getsentry/sentry'
GROUP BY month
ORDER BY month;

-- Top authors by commit count, comparing two calendar years
SELECT author_email,
       SUM(CASE WHEN strftime('%Y', author_date) = '2024' THEN 1 ELSE 0 END) AS commits_2024,
       SUM(CASE WHEN strftime('%Y', author_date) = '2025' THEN 1 ELSE 0 END) AS commits_2025
FROM commits
WHERE repo = 'getsentry/sentry'
  AND strftime('%Y', author_date) IN ('2024', '2025')
GROUP BY author_email
ORDER BY commits_2025 DESC
LIMIT 15;

-- Top PR authors (merged PRs)
SELECT author, COUNT(*) AS prs
FROM pull_requests
WHERE repo = 'getsentry/sentry' AND merged = 1
GROUP BY author ORDER BY prs DESC LIMIT 15;

-- Top reviewers by distinct PRs reviewed
SELECT reviewer, COUNT(DISTINCT pr_number) AS prs_reviewed
FROM pr_reviews
WHERE repo = 'getsentry/sentry' AND state IN ('APPROVED', 'CHANGES_REQUESTED')
GROUP BY reviewer ORDER BY prs_reviewed DESC LIMIT 15;

-- Review balance: ratio of PRs reviewed to PRs authored per person
-- Low ratio = authors more than they review; high = net reviewer
WITH people AS (
    SELECT author AS person FROM pull_requests WHERE repo = 'getsentry/sentry' AND merged = 1
    UNION
    SELECT reviewer AS person FROM pr_reviews WHERE repo = 'getsentry/sentry'
),
authors AS (
    SELECT author AS person, COUNT(*) AS authored
    FROM pull_requests WHERE repo = 'getsentry/sentry' AND merged = 1
    GROUP BY author
),
reviewers AS (
    SELECT reviewer AS person, COUNT(DISTINCT pr_number) AS reviewed
    FROM pr_reviews
    WHERE repo = 'getsentry/sentry' AND state IN ('APPROVED', 'CHANGES_REQUESTED')
    GROUP BY reviewer
)
SELECT
    p.person,
    COALESCE(a.authored, 0) AS prs_authored,
    COALESCE(r.reviewed, 0) AS prs_reviewed,
    ROUND(CAST(COALESCE(r.reviewed, 0) AS REAL) / NULLIF(a.authored, 0), 2) AS review_ratio
FROM people p
LEFT JOIN authors a ON p.person = a.person
LEFT JOIN reviewers r ON p.person = r.person
ORDER BY review_ratio ASC NULLS LAST;

-- Average review turnaround time (hours from PR creation to first review)
SELECT
    pr.author,
    COUNT(*) AS prs,
    ROUND(AVG(
        (julianday(first_review.submitted_at) - julianday(pr.created_at)) * 24
    ), 1) AS avg_hours_to_first_review
FROM pull_requests pr
JOIN (
    SELECT repo, pr_number, MIN(submitted_at) AS submitted_at
    FROM pr_reviews
    GROUP BY repo, pr_number
) first_review ON pr.repo = first_review.repo AND pr.pr_number = first_review.pr_number
WHERE pr.repo = 'getsentry/sentry' AND pr.merged = 1
GROUP BY pr.author
ORDER BY avg_hours_to_first_review DESC;
```
