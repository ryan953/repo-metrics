mod aggregator;
mod analyzers;
mod cli;
mod db;
mod git;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands, Granularity};

/// If a commit changes more files than this, fall back to full tree analysis.
const DELTA_FALLBACK_THRESHOLD: usize = 500;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze(args) => run_analyze(args),
    }
}

fn run_analyze(args: cli::AnalyzeArgs) -> Result<()> {
    let repo = git::open(&args.repo_path)?;
    let mut conn = db::open(&args.db)?;
    db::schema::ensure(&conn)?;

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

    for (i, (commit_sha, commit_date)) in commits.iter().enumerate() {
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
                format!("  ETA {:.0}s", eta_secs)
            } else {
                format!("  ETA {:.0}m", eta_secs / 60.0)
            };
            if rate >= 1.0 {
                format!("  {:.1}/s{}", rate, eta)
            } else {
                format!("  {:.1}/min{}", rate * 60.0, eta)
            }
        } else {
            String::new()
        };

        eprintln!("[{}/{}] {} @ {}{}", i + 1, total, args.repo, &commit_sha[..8], throughput);

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

            let mut file_rows = Vec::with_capacity(file_entries.len());
            for entry in &file_entries {
                let mut stats = analyzers::FileStats::default();
                for analyzer in analyzers {
                    if analyzer.can_analyze(&entry.path) {
                        analyzer.analyze(&entry.path, &entry.content, &mut stats);
                    }
                }
                file_rows.push(build_file_row(&args.repo, commit_sha, commit_date, &entry.path, stats));
            }

            let folder_rows = aggregator::aggregate_to_folders(&file_rows);
            let repo_row =
                aggregator::aggregate_to_repo(&args.repo, commit_sha, commit_date, &file_rows);

            let tx = conn.transaction()?;
            match args.granularity {
                Granularity::All => {
                    db::store::insert_rows(&tx, &file_rows)?;
                    db::store::insert_rows(&tx, &folder_rows)?;
                    db::store::insert_rows(&tx, std::slice::from_ref(&repo_row))?;
                }
                Granularity::Folder => {
                    db::store::insert_rows(&tx, &folder_rows)?;
                    db::store::insert_rows(&tx, std::slice::from_ref(&repo_row))?;
                }
                Granularity::Repo => {
                    db::store::insert_rows(&tx, std::slice::from_ref(&repo_row))?;
                }
            }
            tx.commit()?;
        }

        analyzed += 1;
    }

    eprintln!("Rebuilding unique index...");
    db::schema::ensure_unique_index(&conn)?;

    let elapsed = wall_start.elapsed().as_secs_f64();
    let elapsed_str = if elapsed < 60.0 {
        format!("{:.1}s", elapsed)
    } else if elapsed < 3600.0 {
        format!("{:.1}m", elapsed / 60.0)
    } else {
        format!("{:.1}h", elapsed / 3600.0)
    };
    eprintln!(
        "Done: {} analyzed, {} skipped (already in db), took {}",
        analyzed, skipped, elapsed_str
    );
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

    let diffs = match git::diff_commits(repo, &parent, commit_sha) {
        Ok(d) => d,
        Err(_) => return Ok(false), // missing tree object — fall back to full analysis
    };
    if diffs.len() > DELTA_FALLBACK_THRESHOLD {
        return Ok(false);
    }

    // Analyze old and new versions of every changed file.
    let mut removed: Vec<db::store::StatRow> = Vec::new();
    let mut added: Vec<db::store::StatRow> = Vec::new();

    for diff in &diffs {
        if let Some(old_path) = &diff.old_path {
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
            if let Ok(Some(content)) = git::read_file_at(repo, commit_sha, new_path) {
                let mut stats = analyzers::FileStats::default();
                for analyzer in analyzers {
                    if analyzer.can_analyze(new_path) {
                        analyzer.analyze(new_path, &content, &mut stats);
                    }
                }
                added.push(build_file_row(repo_name, commit_sha, commit_date, new_path, stats));
            }
        }
    }

    match granularity {
        Granularity::Repo => {
            let parent_repo = match db::store::get_repo_row(conn, repo_name, &parent)? {
                Some(r) => r,
                None => return Ok(false),
            };
            let new_repo = aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);
            let tx = conn.unchecked_transaction()?;
            db::store::insert_rows(&tx, std::slice::from_ref(&new_repo))?;
            tx.commit()?;
        }

        Granularity::Folder => {
            let parent_repo = match db::store::get_repo_row(conn, repo_name, &parent)? {
                Some(r) => r,
                None => return Ok(false),
            };

            // Collect the set of folders affected by this diff.
            let affected_folders: std::collections::HashSet<String> = removed
                .iter()
                .chain(added.iter())
                .map(|r| r.folder.clone())
                .collect();
            let affected_vec: Vec<String> = affected_folders.into_iter().collect();

            let parent_folders =
                db::store::get_folder_rows_for_folders(conn, repo_name, &parent, &affected_vec)?;
            if parent_folders.len() != affected_vec.len() {
                // Some affected folder has no parent row — fall back.
                // (Can happen if all files in that folder were added brand-new.)
                // We'll handle this by building missing folder rows from scratch.
                // Build a lookup of what we do have.
                let have: std::collections::HashSet<String> =
                    parent_folders.iter().map(|r| r.folder.clone()).collect();
                let missing: Vec<String> = affected_vec
                    .iter()
                    .filter(|f| !have.contains(*f))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    // If there are genuinely new folders (no parent row), we can still
                    // proceed: treat the missing parent rows as empty.
                    // Build placeholder zeroed rows for missing folders.
                    let _ = missing; // handled below via apply_delta with empty base
                }
            }

            // Build a map from folder → parent folder row (or zeroed placeholder).
            let mut folder_map: std::collections::HashMap<String, db::store::StatRow> =
                parent_folders.into_iter().map(|r| (r.folder.clone(), r)).collect();
            for folder in &affected_vec {
                folder_map.entry(folder.clone()).or_insert_with(|| db::store::StatRow {
                    repo: repo_name.to_string(),
                    commit_sha: parent.clone(),
                    commit_date: String::new(),
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

            // Apply per-folder deltas.
            let mut new_folder_rows: Vec<db::store::StatRow> = Vec::new();
            for (folder, base) in folder_map {
                let folder_removed: Vec<_> =
                    removed.iter().filter(|r| r.folder == folder).cloned().collect();
                let folder_added: Vec<_> =
                    added.iter().filter(|r| r.folder == folder).cloned().collect();
                let updated =
                    aggregator::apply_delta(base, commit_sha, commit_date, &folder_removed, &folder_added);
                if updated.file_count > 0 {
                    new_folder_rows.push(updated);
                }
            }

            let new_repo = aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);

            let tx = conn.unchecked_transaction()?;
            db::store::copy_folder_rows_from_parent(&tx, repo_name, &parent, commit_sha, commit_date, &affected_vec)?;
            db::store::insert_rows(&tx, &new_folder_rows)?;
            db::store::insert_rows(&tx, std::slice::from_ref(&new_repo))?;
            tx.commit()?;
        }

        Granularity::All => {
            let parent_repo = match db::store::get_repo_row(conn, repo_name, &parent)? {
                Some(r) => r,
                None => return Ok(false),
            };

            // Collect all paths that are touched (both old and new) to exclude from SQL copy.
            let exclude_paths: Vec<String> = diffs
                .iter()
                .flat_map(|d| d.old_path.iter().chain(d.new_path.iter()))
                .cloned()
                .collect();
            let exclude_paths: Vec<String> = {
                let mut v = exclude_paths;
                v.sort_unstable();
                v.dedup();
                v
            };

            // Affected folders for the folder-row delta.
            let affected_folders: std::collections::HashSet<String> = removed
                .iter()
                .chain(added.iter())
                .map(|r| r.folder.clone())
                .collect();
            let affected_vec: Vec<String> = affected_folders.into_iter().collect();

            let parent_folders =
                db::store::get_folder_rows_for_folders(conn, repo_name, &parent, &affected_vec)?;
            let mut folder_map: std::collections::HashMap<String, db::store::StatRow> =
                parent_folders.into_iter().map(|r| (r.folder.clone(), r)).collect();
            for folder in &affected_vec {
                folder_map.entry(folder.clone()).or_insert_with(|| db::store::StatRow {
                    repo: repo_name.to_string(),
                    commit_sha: parent.clone(),
                    commit_date: String::new(),
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
                let folder_removed: Vec<_> =
                    removed.iter().filter(|r| r.folder == folder).cloned().collect();
                let folder_added: Vec<_> =
                    added.iter().filter(|r| r.folder == folder).cloned().collect();
                let updated =
                    aggregator::apply_delta(base, commit_sha, commit_date, &folder_removed, &folder_added);
                if updated.file_count > 0 {
                    new_folder_rows.push(updated);
                }
            }

            let new_repo = aggregator::apply_delta(parent_repo, commit_sha, commit_date, &removed, &added);

            let tx = conn.unchecked_transaction()?;
            db::store::copy_file_rows_from_parent(&tx, repo_name, &parent, commit_sha, commit_date, &exclude_paths)?;
            db::store::insert_rows(&tx, &added)?;
            db::store::copy_folder_rows_from_parent(&tx, repo_name, &parent, commit_sha, commit_date, &affected_vec)?;
            db::store::insert_rows(&tx, &new_folder_rows)?;
            db::store::insert_rows(&tx, std::slice::from_ref(&new_repo))?;
            tx.commit()?;
        }
    }

    Ok(true)
}

fn parse_since_date(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let naive = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("Invalid --since date '{}': expected YYYY-MM-DD", s))?;
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

    // Compute type counts before moving stats.file_type into the struct
    let source_file_count = i64::from(stats.file_type.as_deref() == Some("source"));
    let test_file_count = i64::from(stats.file_type.as_deref() == Some("test"));
    let story_file_count = i64::from(stats.file_type.as_deref() == Some("story"));
    let config_file_count = i64::from(stats.file_type.as_deref() == Some("config"));

    // Extension-based file counts
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let py_file_count   = i64::from(matches!(ext.as_str(), "py"));
    let js_file_count   = i64::from(matches!(ext.as_str(), "js" | "mjs" | "cjs"));
    let jsx_file_count  = i64::from(matches!(ext.as_str(), "jsx"));
    let ts_file_count   = i64::from(matches!(ext.as_str(), "ts" | "mts" | "cts"));
    let tsx_file_count  = i64::from(matches!(ext.as_str(), "tsx"));
    let css_file_count  = i64::from(matches!(ext.as_str(), "css" | "scss" | "sass" | "less"));
    let html_file_count = i64::from(matches!(ext.as_str(), "html" | "htm"));
    let md_file_count   = i64::from(matches!(ext.as_str(), "md" | "mdx"));
    let json_file_count = i64::from(matches!(ext.as_str(), "json"));
    let yaml_file_count = i64::from(matches!(ext.as_str(), "yaml" | "yml"));

    db::store::StatRow {
        repo: repo.to_string(),
        commit_sha: commit_sha.to_string(),
        commit_date: commit_date.to_string(),
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
