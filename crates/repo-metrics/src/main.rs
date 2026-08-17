mod aggregator;
mod analyzers;
mod cli;
mod db;
mod git;
mod github;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands, Granularity};

/// If a commit changes more files than this, fall back to full tree analysis.
const DELTA_FALLBACK_THRESHOLD: usize = 500;

/// Number of analyzed commits per database transaction.
const BATCH_SIZE: usize = 100;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze(args) => run_analyze(args),
        Commands::Prs(args) => run_prs(args),
        Commands::Status(args) => run_status(args),
    }
}

fn run_analyze(args: cli::AnalyzeArgs) -> Result<()> {
    let repo = git::open(&args.repo_path)?;
    let conn = db::open(&args.db)?;
    db::schema::ensure(&conn)?;
    db::schema::ensure_commits_table(&conn)?;

    let since = args.since.as_deref().map(parse_since_date).transpose()?;
    let commits = git::all_commits(&repo, &args.commit, since)?;
    let total = commits.len();
    eprintln!("{}: {} commits to process", args.repo, total);

    let analyzers: &[&dyn analyzers::Analyzer] = &[
        &analyzers::common::CommonAnalyzer,
        &analyzers::javascript::JsAnalyzer,
        &analyzers::python::PyAnalyzer,
    ];

    let mut analyzed = 0usize;
    let mut skipped = 0usize;
    let mut analysis_start: Option<std::time::Instant> = None;
    let wall_start = std::time::Instant::now();

    // Drop the unique index before bulk inserts; it will be rebuilt once at the end.
    // The lightweight idx_commit_lookup index is kept so commit_exists stays fast.
    db::schema::drop_unique_index(&conn)?;

    conn.execute_batch("BEGIN")?;

    for (i, (commit_sha, commit_date)) in commits.iter().enumerate() {
        // Commit metadata capture is decoupled from the stats skip-check below: it's
        // cheap (one object lookup, no diffing) and idempotent (INSERT OR IGNORE), so we
        // always ensure a `commits` row exists — including for commits whose `stats` work
        // is about to be skipped. This is what lets upgrading and re-running `analyze` on
        // an already-fully-analyzed database backfill `commits` without a full re-walk.
        let meta = git::commit_meta(&repo, commit_sha)?;
        db::commit_store::insert_commit(
            &conn,
            &db::commit_store::CommitRow {
                repo: args.repo.clone(),
                sha: commit_sha.clone(),
                author_date: meta.author_date,
                author_name: meta.author_name,
                author_email: meta.author_email,
                committer_date: meta.committer_date,
                committer_name: meta.committer_name,
                committer_email: meta.committer_email,
            },
        )?;

        if db::store::commit_exists(&conn, &args.repo, commit_sha)? {
            skipped += 1;
            continue;
        }

        // Start the throughput clock on the first commit we actually analyze,
        // so skipped commits don't dilute the rate.
        let start = analysis_start.get_or_insert_with(std::time::Instant::now);
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = if analyzed > 0 && elapsed > 0.0 {
            let rate = analyzed as f64 / elapsed;
            let remaining = (total - i) as f64; // commits not yet processed
            let eta_secs = remaining / rate;
            let eta = if eta_secs < 60.0 {
                format!("  ETA {eta_secs:.0}s")
            } else {
                format!("  ETA {:.0}m", eta_secs / 60.0)
            };
            if rate >= 1.0 {
                format!("  {rate:.1}/s{eta}")
            } else {
                format!("  {:.1}/min{}", rate * 60.0, eta)
            }
        } else {
            String::new()
        };

        eprintln!(
            "[{}/{}] {} @ {}{}",
            i + 1,
            total,
            args.repo,
            &commit_sha[..8],
            throughput
        );

        let used_delta = try_delta_analyze(
            &repo,
            &conn,
            analyzers,
            &args.repo,
            commit_sha,
            commit_date,
            &args.granularity,
        )?;

        if !used_delta {
            let file_entries = match git::walk_files(&repo, commit_sha) {
                Ok(entries) => entries,
                Err(e) => {
                    eprintln!("  skipping {}: {}", &commit_sha[..8], e);
                    skipped += 1;
                    continue;
                }
            };

            // Full-tree analysis: close all open rows, then insert fresh ones.
            db::store::close_all_open_rows(&conn, &args.repo, commit_sha, commit_date)?;

            let mut file_rows = Vec::with_capacity(file_entries.len());
            for entry in &file_entries {
                let mut stats = analyzers::FileStats::default();
                for analyzer in analyzers {
                    if analyzer.can_analyze(&entry.path) {
                        analyzer.analyze(&entry.path, &entry.content, &mut stats);
                    }
                }
                file_rows.push(build_file_row(
                    &args.repo,
                    commit_sha,
                    commit_date,
                    &entry.path,
                    stats,
                ));
            }

            let folder_rows = aggregator::aggregate_to_folders(&file_rows);
            let repo_row =
                aggregator::aggregate_to_repo(&args.repo, commit_sha, commit_date, &file_rows);

            match args.granularity {
                Granularity::All => {
                    db::store::insert_rows(&conn, &file_rows)?;
                    db::store::insert_rows(&conn, &folder_rows)?;
                    db::store::insert_rows(&conn, std::slice::from_ref(&repo_row))?;
                }
                Granularity::Folder => {
                    db::store::insert_rows(&conn, &folder_rows)?;
                    db::store::insert_rows(&conn, std::slice::from_ref(&repo_row))?;
                }
                Granularity::Repo => {
                    db::store::insert_rows(&conn, std::slice::from_ref(&repo_row))?;
                }
            }
        }

        analyzed += 1;
        // Batched on total iterations (not just `analyzed`) so a run that's mostly or
        // entirely backfilling `commits` for already-analyzed history — where `analyzed`
        // may never advance — still commits the transaction periodically instead of
        // holding tens of thousands of commit-metadata inserts open at once.
        if (i + 1).is_multiple_of(BATCH_SIZE) {
            conn.execute_batch("COMMIT; BEGIN")?;
        }
    }

    conn.execute_batch("COMMIT")?;

    eprintln!("Rebuilding unique index...");
    db::schema::ensure_unique_index(&conn)?;

    let elapsed = wall_start.elapsed().as_secs_f64();
    let elapsed_str = if elapsed < 60.0 {
        format!("{elapsed:.1}s")
    } else if elapsed < 3600.0 {
        format!("{:.1}m", elapsed / 60.0)
    } else {
        format!("{:.1}h", elapsed / 3600.0)
    };
    eprintln!("Done: {analyzed} analyzed, {skipped} skipped (already in db), took {elapsed_str}");
    Ok(())
}

/// Attempts to analyze a commit using a delta from its parent.
///
/// Returns `Ok(true)` on success, `Ok(false)` if delta is not applicable (root commit,
/// parent not in DB, too many changed files) — caller should fall back to full analysis.
fn try_delta_analyze(
    repo: &git2::Repository,
    conn: &rusqlite::Connection,
    analyzers: &[&dyn analyzers::Analyzer],
    repo_name: &str,
    commit_sha: &str,
    commit_date: &str,
    granularity: &Granularity,
) -> Result<bool> {
    let parent = match git::parent_sha(repo, commit_sha) {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(false), // root commit
        Err(_) => return Ok(false),   // inaccessible commit object
    };

    if !db::store::commit_exists(conn, repo_name, &parent)? {
        return Ok(false);
    }

    let diffs = match git::diff_commits(repo, &parent, commit_sha) {
        Ok(d) => d,
        Err(_) => return Ok(false),
    };
    if diffs.len() > DELTA_FALLBACK_THRESHOLD {
        return Ok(false);
    }

    let mut added: Vec<db::store::StatRow> = Vec::new();
    let mut changed_file_paths: Vec<String> = Vec::new();
    let mut removed: Vec<db::store::StatRow> = Vec::new();

    for diff in &diffs {
        if let Some(old_path) = &diff.old_path {
            changed_file_paths.push(old_path.clone());
            if let Ok(Some(content)) = git::read_file_at(repo, &parent, old_path) {
                let mut stats = analyzers::FileStats::default();
                for analyzer in analyzers {
                    if analyzer.can_analyze(old_path) {
                        analyzer.analyze(old_path, &content, &mut stats);
                    }
                }
                removed.push(build_file_row(repo_name, &parent, "", old_path, stats));
            }
        }
        if let Some(new_path) = &diff.new_path {
            if !changed_file_paths.contains(new_path) {
                changed_file_paths.push(new_path.clone());
            }
            if let Ok(Some(content)) = git::read_file_at(repo, commit_sha, new_path) {
                let mut stats = analyzers::FileStats::default();
                for analyzer in analyzers {
                    if analyzer.can_analyze(new_path) {
                        analyzer.analyze(new_path, &content, &mut stats);
                    }
                }
                added.push(build_file_row(
                    repo_name,
                    commit_sha,
                    commit_date,
                    new_path,
                    stats,
                ));
            }
        }
    }

    changed_file_paths.sort_unstable();
    changed_file_paths.dedup();

    match granularity {
        Granularity::Repo => {
            let parent_repo = match db::store::get_current_repo_row(conn, repo_name)? {
                Some(r) => r,
                None => return Ok(false),
            };

            db::store::close_repo_row(conn, repo_name, commit_sha, commit_date)?;
            let new_repo =
                aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);
            db::store::insert_rows(conn, std::slice::from_ref(&new_repo))?;
        }

        Granularity::Folder => {
            let parent_repo = match db::store::get_current_repo_row(conn, repo_name)? {
                Some(r) => r,
                None => return Ok(false),
            };

            let affected_folders: std::collections::HashSet<String> = removed
                .iter()
                .chain(added.iter())
                .map(|r| r.folder.clone())
                .collect();
            let affected_vec: Vec<String> = affected_folders.into_iter().collect();

            let parent_folders =
                db::store::get_folder_rows_for_folders(conn, repo_name, &affected_vec)?;

            let mut folder_map: std::collections::HashMap<String, db::store::StatRow> =
                parent_folders
                    .into_iter()
                    .map(|r| (r.folder.clone(), r))
                    .collect();
            for folder in &affected_vec {
                folder_map
                    .entry(folder.clone())
                    .or_insert_with(|| db::store::StatRow {
                        repo: repo_name.to_string(),
                        valid_from_sha: parent.clone(),
                        valid_from_date: String::new(),
                        valid_to_sha: None,
                        valid_to_date: None,
                        row_type: "folder".to_string(),
                        folder: folder.clone(),
                        folder_depth: folder.split('/').filter(|s| !s.is_empty()).count() as i32,
                        file_name: None,
                        file_count: 0,
                        sloc_nonblank: 0,
                        sloc_noncomment: 0,
                        file_type: None,
                        source_file_count: 0,
                        test_file_count: 0,
                        story_file_count: 0,
                        config_file_count: 0,
                        js_exports_default: None,
                        js_exports_named: None,
                        js_exports_total: None,
                        js_export_matches_filename: 0,
                        py_file_count: 0,
                        js_file_count: 0,
                        jsx_file_count: 0,
                        ts_file_count: 0,
                        tsx_file_count: 0,
                        css_file_count: 0,
                        html_file_count: 0,
                        md_file_count: 0,
                        json_file_count: 0,
                        yaml_file_count: 0,
                    });
            }

            let mut new_folder_rows: Vec<db::store::StatRow> = Vec::new();
            for (folder, base) in folder_map {
                let folder_removed: Vec<_> = removed
                    .iter()
                    .filter(|r| r.folder == folder)
                    .cloned()
                    .collect();
                let folder_added: Vec<_> = added
                    .iter()
                    .filter(|r| r.folder == folder)
                    .cloned()
                    .collect();
                let updated = aggregator::apply_delta(
                    base,
                    commit_sha,
                    commit_date,
                    &folder_removed,
                    &folder_added,
                );
                if updated.file_count > 0 {
                    new_folder_rows.push(updated);
                }
            }

            let new_repo =
                aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);

            db::store::close_rows_for_folders(
                conn,
                repo_name,
                commit_sha,
                commit_date,
                &affected_vec,
            )?;
            db::store::insert_rows(conn, &new_folder_rows)?;

            db::store::close_repo_row(conn, repo_name, commit_sha, commit_date)?;
            db::store::insert_rows(conn, std::slice::from_ref(&new_repo))?;
        }

        Granularity::All => {
            let parent_repo = match db::store::get_current_repo_row(conn, repo_name)? {
                Some(r) => r,
                None => return Ok(false),
            };

            let affected_folders: std::collections::HashSet<String> = removed
                .iter()
                .chain(added.iter())
                .map(|r| r.folder.clone())
                .collect();
            let affected_vec: Vec<String> = affected_folders.into_iter().collect();

            let parent_folders =
                db::store::get_folder_rows_for_folders(conn, repo_name, &affected_vec)?;
            let mut folder_map: std::collections::HashMap<String, db::store::StatRow> =
                parent_folders
                    .into_iter()
                    .map(|r| (r.folder.clone(), r))
                    .collect();
            for folder in &affected_vec {
                folder_map
                    .entry(folder.clone())
                    .or_insert_with(|| db::store::StatRow {
                        repo: repo_name.to_string(),
                        valid_from_sha: parent.clone(),
                        valid_from_date: String::new(),
                        valid_to_sha: None,
                        valid_to_date: None,
                        row_type: "folder".to_string(),
                        folder: folder.clone(),
                        folder_depth: folder.split('/').filter(|s| !s.is_empty()).count() as i32,
                        file_name: None,
                        file_count: 0,
                        sloc_nonblank: 0,
                        sloc_noncomment: 0,
                        file_type: None,
                        source_file_count: 0,
                        test_file_count: 0,
                        story_file_count: 0,
                        config_file_count: 0,
                        js_exports_default: None,
                        js_exports_named: None,
                        js_exports_total: None,
                        js_export_matches_filename: 0,
                        py_file_count: 0,
                        js_file_count: 0,
                        jsx_file_count: 0,
                        ts_file_count: 0,
                        tsx_file_count: 0,
                        css_file_count: 0,
                        html_file_count: 0,
                        md_file_count: 0,
                        json_file_count: 0,
                        yaml_file_count: 0,
                    });
            }

            let mut new_folder_rows: Vec<db::store::StatRow> = Vec::new();
            for (folder, base) in folder_map {
                let folder_removed: Vec<_> = removed
                    .iter()
                    .filter(|r| r.folder == folder)
                    .cloned()
                    .collect();
                let folder_added: Vec<_> = added
                    .iter()
                    .filter(|r| r.folder == folder)
                    .cloned()
                    .collect();
                let updated = aggregator::apply_delta(
                    base,
                    commit_sha,
                    commit_date,
                    &folder_removed,
                    &folder_added,
                );
                if updated.file_count > 0 {
                    new_folder_rows.push(updated);
                }
            }

            let new_repo =
                aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);

            db::store::close_rows_for_file_paths(
                conn,
                repo_name,
                commit_sha,
                commit_date,
                &changed_file_paths,
            )?;
            db::store::insert_rows(conn, &added)?;

            db::store::close_rows_for_folders(
                conn,
                repo_name,
                commit_sha,
                commit_date,
                &affected_vec,
            )?;
            db::store::insert_rows(conn, &new_folder_rows)?;

            db::store::close_repo_row(conn, repo_name, commit_sha, commit_date)?;
            db::store::insert_rows(conn, std::slice::from_ref(&new_repo))?;
        }
    }

    Ok(true)
}

fn run_status(args: cli::StatusArgs) -> Result<()> {
    let repo = git::open(&args.repo_path)?;
    let conn = db::open(&args.db)?;

    let since = args.since.as_deref().map(parse_since_date).transpose()?;
    let commits = git::all_commits(&repo, &args.commit, since)?;
    let in_db = db::store::committed_shas(&conn, &args.repo)?;

    let total = commits.len();
    let loaded = commits
        .iter()
        .filter(|(sha, _)| in_db.contains(sha))
        .count();
    let missing = total - loaded;

    eprintln!("{}", args.repo);
    eprintln!("  commits in git history: {total}");
    eprintln!("  loaded in db:           {loaded}");
    eprintln!("  missing:                {missing}");

    if total > 0 {
        let pct = loaded as f64 / total as f64 * 100.0;
        eprintln!("  coverage:               {pct:.1}%");
    }

    if let (Some(oldest), Some(newest)) = (commits.first(), commits.last()) {
        let oldest_date = &oldest.1;
        let newest_date = &newest.1;
        eprintln!(
            "  date range (git):       {} .. {}",
            &oldest_date[..10],
            &newest_date[..10]
        );
    }

    let loaded_commits: Vec<&(String, String)> = commits
        .iter()
        .filter(|(sha, _)| in_db.contains(sha))
        .collect();
    if let (Some(oldest), Some(newest)) = (loaded_commits.first(), loaded_commits.last()) {
        eprintln!(
            "  date range (db):        {} .. {}",
            &oldest.1[..10],
            &newest.1[..10]
        );
    }

    if missing > 0 {
        let missing_commits: Vec<&(String, String)> = commits
            .iter()
            .filter(|(sha, _)| !in_db.contains(sha))
            .collect();
        let show = missing_commits.len().min(10);
        eprintln!("\n  oldest missing commits:");
        for (sha, date) in &missing_commits[..show] {
            eprintln!("    {} {}", &sha[..8], &date[..10]);
        }
        if missing_commits.len() > show {
            eprintln!("    ... and {} more", missing_commits.len() - show);
        }
    }

    Ok(())
}

fn run_prs(args: cli::PrsArgs) -> Result<()> {
    let conn = db::open(&args.db)?;
    db::schema::ensure_pr_tables(&conn)?;

    let client = github::GitHubClient::new(&args.token, &args.repo)?;

    let (remaining, limit) = client.rate_limit_remaining()?;
    eprintln!("GitHub API rate limit: {remaining}/{limit} remaining");

    let since_str = args.since.as_deref();
    eprintln!("{}: Fetching pull requests...", args.repo);
    let pulls = client.list_pulls(since_str)?;
    eprintln!("{}: Found {} pull requests", args.repo, pulls.len());

    let wall_start = std::time::Instant::now();
    let mut fetched = 0usize;
    let mut skipped = 0usize;

    conn.execute_batch("BEGIN")?;

    for (i, pr) in pulls.iter().enumerate() {
        if let Some(db_updated) = db::pr_store::pr_updated_at(&conn, &args.repo, pr.number)? {
            if db_updated == pr.updated_at {
                skipped += 1;
                continue;
            }
        }

        let title_preview: String = pr.title.chars().take(60).collect();
        eprintln!(
            "[{}/{}] PR #{}: {}",
            i + 1,
            pulls.len(),
            pr.number,
            title_preview
        );

        let detail = client.get_pull(pr.number)?;

        let pr_row = db::pr_store::PullRequestRow {
            repo: args.repo.clone(),
            pr_number: detail.number,
            title: detail.title,
            author: detail.user.login,
            state: detail.state.clone(),
            draft: detail.draft.unwrap_or(false),
            created_at: detail.created_at,
            updated_at: detail.updated_at,
            merged_at: detail.merged_at,
            closed_at: detail.closed_at,
            merged: detail.merged.unwrap_or(false),
            additions: detail.additions,
            deletions: detail.deletions,
            changed_files: detail.changed_files,
            base_ref: Some(detail.base.ref_name),
            head_ref: Some(detail.head.ref_name),
        };
        db::pr_store::upsert_pull_request(&conn, &pr_row)?;

        let reviews = client.list_reviews(pr.number)?;
        for review in &reviews {
            if let (Some(user), Some(submitted_at)) = (&review.user, &review.submitted_at) {
                let review_row = db::pr_store::ReviewRow {
                    repo: args.repo.clone(),
                    pr_number: pr.number,
                    review_id: review.id,
                    reviewer: user.login.clone(),
                    state: review.state.clone(),
                    submitted_at: submitted_at.clone(),
                };
                db::pr_store::upsert_review(&conn, &review_row)?;
            }
        }

        fetched += 1;
        if fetched.is_multiple_of(100) {
            conn.execute_batch("COMMIT; BEGIN")?;
        }
    }

    conn.execute_batch("COMMIT")?;

    let elapsed = wall_start.elapsed().as_secs_f64();
    let elapsed_str = if elapsed < 60.0 {
        format!("{elapsed:.1}s")
    } else {
        format!("{:.1}m", elapsed / 60.0)
    };
    eprintln!("Done: {fetched} fetched, {skipped} skipped (unchanged), took {elapsed_str}");

    print_pr_summary(&conn, &args.repo)?;

    Ok(())
}

fn print_pr_summary(conn: &rusqlite::Connection, repo: &str) -> Result<()> {
    use rusqlite::params;

    eprintln!("\n--- PR Authors (merged, top 15) ---");
    let mut stmt = conn.prepare(
        "SELECT author, COUNT(*) AS cnt
         FROM pull_requests WHERE repo = ?1 AND merged = 1
         GROUP BY author ORDER BY cnt DESC LIMIT 15",
    )?;
    let rows = stmt.query_map(params![repo], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (author, count) = row?;
        eprintln!("  {count:>5}  {author}");
    }

    eprintln!("\n--- PR Reviewers (top 15) ---");
    let mut stmt = conn.prepare(
        "SELECT reviewer, COUNT(DISTINCT pr_number) AS cnt
         FROM pr_reviews WHERE repo = ?1 AND state IN ('APPROVED', 'CHANGES_REQUESTED', 'COMMENTED')
         GROUP BY reviewer ORDER BY cnt DESC LIMIT 15",
    )?;
    let rows = stmt.query_map(params![repo], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (reviewer, count) = row?;
        eprintln!("  {count:>5}  {reviewer}");
    }

    eprintln!("\n--- Review Balance (reviews / authored, merged PRs) ---");
    eprintln!("  {:>5}  {:>5}  {:>6}  person", "auth", "revw", "ratio");
    let mut stmt = conn.prepare(
        "WITH people AS (
            SELECT author AS person FROM pull_requests WHERE repo = ?1 AND merged = 1
            UNION
            SELECT reviewer AS person FROM pr_reviews WHERE repo = ?1
         ),
         authors AS (
            SELECT author AS person, COUNT(*) AS authored
            FROM pull_requests WHERE repo = ?1 AND merged = 1
            GROUP BY author
         ),
         reviewers AS (
            SELECT reviewer AS person, COUNT(DISTINCT pr_number) AS reviewed
            FROM pr_reviews
            WHERE repo = ?1 AND state IN ('APPROVED', 'CHANGES_REQUESTED')
            GROUP BY reviewer
         )
         SELECT
            p.person,
            COALESCE(a.authored, 0) AS authored,
            COALESCE(r.reviewed, 0) AS reviewed,
            CASE
                WHEN COALESCE(a.authored, 0) = 0 THEN NULL
                ELSE ROUND(CAST(COALESCE(r.reviewed, 0) AS REAL) / a.authored, 2)
            END AS ratio
         FROM people p
         LEFT JOIN authors a ON p.person = a.person
         LEFT JOIN reviewers r ON p.person = r.person
         WHERE COALESCE(a.authored, 0) > 0 OR COALESCE(r.reviewed, 0) > 0
         ORDER BY ratio ASC NULLS LAST",
    )?;
    let rows = stmt.query_map(params![repo], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<f64>>(3)?,
        ))
    })?;
    for row in rows {
        let (person, authored, reviewed, ratio) = row?;
        let ratio_str = match ratio {
            Some(r) => format!("{r:.2}"),
            None => "  -".to_string(),
        };
        eprintln!("  {authored:>5}  {reviewed:>5}  {ratio_str:>6}  {person}");
    }

    Ok(())
}

fn parse_since_date(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let naive = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("Invalid --since date '{s}': expected YYYY-MM-DD"))?;
    Ok(naive.and_hms_opt(0, 0, 0).unwrap().and_utc())
}

fn build_file_row(
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
    path: &str,
    stats: analyzers::FileStats,
) -> db::store::StatRow {
    let (folder, folder_depth) = git::path_folder(path);

    let source_file_count = i64::from(stats.file_type.as_deref() == Some("source"));
    let test_file_count = i64::from(stats.file_type.as_deref() == Some("test"));
    let story_file_count = i64::from(stats.file_type.as_deref() == Some("story"));
    let config_file_count = i64::from(stats.file_type.as_deref() == Some("config"));

    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let py_file_count = i64::from(matches!(ext.as_str(), "py"));
    let js_file_count = i64::from(matches!(ext.as_str(), "js" | "mjs" | "cjs"));
    let jsx_file_count = i64::from(matches!(ext.as_str(), "jsx"));
    let ts_file_count = i64::from(matches!(ext.as_str(), "ts" | "mts" | "cts"));
    let tsx_file_count = i64::from(matches!(ext.as_str(), "tsx"));
    let css_file_count = i64::from(matches!(ext.as_str(), "css" | "scss" | "sass" | "less"));
    let html_file_count = i64::from(matches!(ext.as_str(), "html" | "htm"));
    let md_file_count = i64::from(matches!(ext.as_str(), "md" | "mdx"));
    let json_file_count = i64::from(matches!(ext.as_str(), "json"));
    let yaml_file_count = i64::from(matches!(ext.as_str(), "yaml" | "yml"));

    db::store::StatRow {
        repo: repo.to_string(),
        valid_from_sha: commit_sha.to_string(),
        valid_from_date: commit_date.to_string(),
        valid_to_sha: None,
        valid_to_date: None,
        row_type: "file".to_string(),
        folder,
        folder_depth,
        file_name: Some(path.to_string()),
        file_count: 1,
        sloc_nonblank: stats.sloc_nonblank as i64,
        sloc_noncomment: stats.sloc_noncomment as i64,
        file_type: stats.file_type,
        source_file_count,
        test_file_count,
        story_file_count,
        config_file_count,
        js_exports_default: stats.js_exports_default.map(|v| v as i64),
        js_exports_named: stats.js_exports_named.map(|v| v as i64),
        js_exports_total: stats.js_exports_total.map(|v| v as i64),
        js_export_matches_filename: i64::from(stats.js_export_matches_filename),
        py_file_count,
        js_file_count,
        jsx_file_count,
        ts_file_count,
        tsx_file_count,
        css_file_count,
        html_file_count,
        md_file_count,
        json_file_count,
        yaml_file_count,
    }
}
