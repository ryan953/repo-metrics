//! Integration tests for the `commits` table populated by `analyze`.
//!
//! Builds a tiny throwaway git repo on disk, runs the compiled `repo-metrics` binary
//! against it, and inspects the resulting SQLite database directly.

use std::path::Path;
use std::process::Command;

const REPO_ID: &str = "test-org/test-repo";

fn git(repo_dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed");
}

struct CommitIdentity<'a> {
    file_name: &'a str,
    contents: &'a str,
    author_name: &'a str,
    author_email: &'a str,
    author_unix_ts: i64,
    committer_name: &'a str,
    committer_email: &'a str,
    committer_unix_ts: i64,
}

fn commit(repo_dir: &Path, id: &CommitIdentity) {
    std::fs::write(repo_dir.join(id.file_name), id.contents).expect("failed to write file");
    git(repo_dir, &["add", id.file_name]);

    let status = Command::new("git")
        .args(["commit", "-m", &format!("add {}", id.file_name)])
        .current_dir(repo_dir)
        .env("GIT_AUTHOR_NAME", id.author_name)
        .env("GIT_AUTHOR_EMAIL", id.author_email)
        .env("GIT_AUTHOR_DATE", format!("{} +0000", id.author_unix_ts))
        .env("GIT_COMMITTER_NAME", id.committer_name)
        .env("GIT_COMMITTER_EMAIL", id.committer_email)
        .env(
            "GIT_COMMITTER_DATE",
            format!("{} +0000", id.committer_unix_ts),
        )
        .status()
        .expect("failed to run git commit");
    assert!(status.success(), "git commit failed");
}

fn iso(unix_ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(unix_ts, 0)
        .single()
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn init_repo(repo_dir: &Path) {
    git(repo_dir, &["init", "-q", "-b", "main"]);
    git(repo_dir, &["config", "user.name", "Default User"]);
    git(repo_dir, &["config", "user.email", "default@example.com"]);

    // Commit 1: author == committer.
    commit(
        repo_dir,
        &CommitIdentity {
            file_name: "a.txt",
            contents: "hello\n",
            author_name: "Alice",
            author_email: "alice@example.com",
            author_unix_ts: 1_700_000_000,
            committer_name: "Alice",
            committer_email: "alice@example.com",
            committer_unix_ts: 1_700_000_000,
        },
    );

    // Commit 2: another normal commit from a different person.
    commit(
        repo_dir,
        &CommitIdentity {
            file_name: "b.txt",
            contents: "world\n",
            author_name: "Bob",
            author_email: "bob@example.com",
            author_unix_ts: 1_700_003_600,
            committer_name: "Bob",
            committer_email: "bob@example.com",
            committer_unix_ts: 1_700_003_600,
        },
    );

    // Commit 3: author != committer, simulating a squash-merge / rebase where the
    // original author's identity is preserved but the commit is created by someone
    // (or something, e.g. a CI bot) else.
    commit(
        repo_dir,
        &CommitIdentity {
            file_name: "c.txt",
            contents: "squashed\n",
            author_name: "Carol",
            author_email: "carol@example.com",
            author_unix_ts: 1_690_000_000,
            committer_name: "CI Bot",
            committer_email: "ci-bot@example.com",
            committer_unix_ts: 1_700_007_200,
        },
    );
}

fn run_analyze(repo_dir: &Path, db_path: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_repo-metrics"))
        .args([
            "analyze",
            repo_dir.to_str().unwrap(),
            "--repo",
            REPO_ID,
            "--db",
            db_path.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run repo-metrics analyze");
    assert!(status.success(), "repo-metrics analyze failed");
}

struct CommitRow {
    sha: String,
    author_date: String,
    author_name: String,
    author_email: String,
    committer_date: String,
    committer_name: String,
    committer_email: String,
}

fn read_commits(db_path: &Path) -> Vec<CommitRow> {
    let conn = rusqlite::Connection::open(db_path).expect("failed to open db");
    let mut stmt = conn
        .prepare(
            "SELECT sha, author_date, author_name, author_email,
                    committer_date, committer_name, committer_email
             FROM commits WHERE repo = ?1 ORDER BY author_date",
        )
        .unwrap();
    let rows = stmt
        .query_map([REPO_ID], |row| {
            Ok(CommitRow {
                sha: row.get(0)?,
                author_date: row.get(1)?,
                author_name: row.get(2)?,
                author_email: row.get(3)?,
                committer_date: row.get(4)?,
                committer_name: row.get(5)?,
                committer_email: row.get(6)?,
            })
        })
        .unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

fn count_stats_rows(db_path: &Path) -> i64 {
    let conn = rusqlite::Connection::open(db_path).expect("failed to open db");
    conn.query_row(
        "SELECT COUNT(*) FROM stats WHERE repo = ?1",
        [REPO_ID],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn analyze_populates_commits_table_with_author_and_committer_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    init_repo(&repo_dir);

    let db_path = tmp.path().join("metrics.db");
    run_analyze(&repo_dir, &db_path);

    let commits = read_commits(&db_path);
    assert_eq!(commits.len(), 3, "expected one row per commit");

    // Oldest author_date first: Carol (rebased/squashed), then Alice, then Bob.
    assert_eq!(commits[0].author_name, "Carol");
    assert_eq!(commits[0].author_email, "carol@example.com");
    assert_eq!(commits[0].author_date, iso(1_690_000_000));
    // Author and committer differ for the squash-style commit.
    assert_eq!(commits[0].committer_name, "CI Bot");
    assert_eq!(commits[0].committer_email, "ci-bot@example.com");
    assert_eq!(commits[0].committer_date, iso(1_700_007_200));
    assert_eq!(commits[0].sha.len(), 40);

    assert_eq!(commits[1].author_name, "Alice");
    assert_eq!(commits[1].author_email, "alice@example.com");
    assert_eq!(commits[1].author_date, iso(1_700_000_000));
    assert_eq!(commits[1].committer_name, "Alice");
    assert_eq!(commits[1].committer_email, "alice@example.com");
    assert_eq!(commits[1].committer_date, iso(1_700_000_000));

    assert_eq!(commits[2].author_name, "Bob");
    assert_eq!(commits[2].author_email, "bob@example.com");
    assert_eq!(commits[2].author_date, iso(1_700_003_600));
}

#[test]
fn rerunning_analyze_is_idempotent_for_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    init_repo(&repo_dir);

    let db_path = tmp.path().join("metrics.db");
    run_analyze(&repo_dir, &db_path);
    assert_eq!(read_commits(&db_path).len(), 3);

    // Second run should not duplicate rows.
    run_analyze(&repo_dir, &db_path);
    assert_eq!(read_commits(&db_path).len(), 3);
}

/// Simulates upgrading `repo-metrics` on a database that already has full `stats`
/// history from an older version that had no `commits` table: delete all `commits` rows
/// (as if the table never existed) but leave `stats` populated, then re-run `analyze`.
/// The re-run must backfill `commits` for the whole history without redoing the
/// expensive per-file/folder `stats` aggregation for commits already recorded there.
#[test]
fn rerunning_analyze_backfills_commits_without_redoing_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    init_repo(&repo_dir);

    let db_path = tmp.path().join("metrics.db");
    run_analyze(&repo_dir, &db_path);
    assert_eq!(read_commits(&db_path).len(), 3);
    let stats_rows_before = count_stats_rows(&db_path);
    assert!(stats_rows_before > 0);

    // Simulate a pre-existing database from before the `commits` table existed.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute("DELETE FROM commits", []).unwrap();
    }
    assert_eq!(read_commits(&db_path).len(), 0);

    run_analyze(&repo_dir, &db_path);

    // commits backfilled for the entire history...
    assert_eq!(read_commits(&db_path).len(), 3);
    // ...without re-running the (skipped) stats aggregation for those same commits.
    assert_eq!(count_stats_rows(&db_path), stats_rows_before);
}
