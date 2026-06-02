use anyhow::{Context, Result};
use chrono::TimeZone;
use git2::{Delta, ObjectType, Oid, Repository, Sort, TreeWalkMode, TreeWalkResult};
use std::path::Path;

pub struct FileEntry {
    pub path: String,
    pub content: String,
}

/// One entry from a diff between two commits.
/// `old_path` is None for added files; `new_path` is None for deleted files.
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
}

pub fn open(path: &Path) -> Result<Repository> {
    Repository::open(path)
        .with_context(|| format!("Failed to open git repository at {:?}", path))
}

/// Resolve a refspec (branch, tag, SHA, "HEAD") to its commit SHA and ISO date.
#[allow(dead_code)]
pub fn resolve_commit(repo: &Repository, spec: &str) -> Result<(String, String)> {
    let obj = repo
        .revparse_single(spec)
        .with_context(|| format!("Failed to resolve '{}'", spec))?;
    let commit = obj
        .peel_to_commit()
        .with_context(|| format!("'{}' does not point to a commit", spec))?;
    Ok(commit_sha_and_date(&commit))
}

/// Walk all ancestors of `tip_spec` (oldest → newest) and return their (sha, date) pairs.
///
/// If `since` is provided, commits older than that timestamp are excluded. The walk
/// proceeds newest-first and stops as soon as a commit predates `since`, avoiding a
/// full traversal of long histories. The result is reversed to oldest-first so that
/// partial runs leave the database in a consistent historical state.
pub fn all_commits(
    repo: &Repository,
    tip_spec: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Vec<(String, String)>> {
    let tip_obj = repo
        .revparse_single(tip_spec)
        .with_context(|| format!("Failed to resolve '{}'", tip_spec))?;
    let tip_oid = tip_obj
        .peel_to_commit()
        .with_context(|| format!("'{}' does not point to a commit", tip_spec))?
        .id();

    let mut revwalk = repo.revwalk()?;
    revwalk.push(tip_oid)?;
    // Newest-first so we can break early when we pass the `since` cutoff
    revwalk.set_sorting(Sort::TIME)?;

    let mut commits = Vec::new();
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if let Some(cutoff) = since {
            let commit_time = chrono::Utc
                .timestamp_opt(commit.time().seconds(), 0)
                .single()
                .unwrap_or_default();
            if commit_time < cutoff {
                break;
            }
        }
        commits.push(commit_sha_and_date(&commit));
    }

    commits.reverse(); // oldest → newest for consistent incremental runs
    Ok(commits)
}

fn commit_sha_and_date(commit: &git2::Commit<'_>) -> (String, String) {
    let sha = commit.id().to_string();
    let date = chrono::Utc
        .timestamp_opt(commit.time().seconds(), 0)
        .single()
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    (sha, date)
}

pub fn walk_files(repo: &Repository, commit_sha: &str) -> Result<Vec<FileEntry>> {
    let oid = Oid::from_str(commit_sha)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    // Collect (path, oid) pairs first to avoid borrow conflicts in the closure
    let mut blob_entries: Vec<(String, Oid)> = Vec::new();
    tree.walk(TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(ObjectType::Blob) {
            if let Some(name) = entry.name() {
                blob_entries.push((format!("{}{}", root, name), entry.id()));
            }
        }
        TreeWalkResult::Ok
    })?;

    let mut files = Vec::with_capacity(blob_entries.len());
    for (path, oid) in blob_entries {
        let blob = match repo.find_blob(oid) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let raw = blob.content();
        if raw.contains(&0u8) {
            // Binary file — skip
            continue;
        }
        if let Ok(content) = std::str::from_utf8(raw) {
            files.push(FileEntry {
                path,
                content: content.to_string(),
            });
        }
    }

    Ok(files)
}

/// Returns the SHA of the first parent of `commit_sha`, or None for root commits.
pub fn parent_sha(repo: &Repository, commit_sha: &str) -> Result<Option<String>> {
    let oid = Oid::from_str(commit_sha)?;
    let commit = repo.find_commit(oid)?;
    Ok(commit.parent_ids().next().map(|p| p.to_string()))
}

/// Returns the list of file changes between two commits (old → new).
pub fn diff_commits(repo: &Repository, old_sha: &str, new_sha: &str) -> Result<Vec<FileDiff>> {
    let old_tree = repo.find_commit(Oid::from_str(old_sha)?)?.tree()?;
    let new_tree = repo.find_commit(Oid::from_str(new_sha)?)?.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;

    let mut result = Vec::new();
    for delta in diff.deltas() {
        let old_path = match delta.status() {
            Delta::Added | Delta::Copied => None,
            _ => delta
                .old_file()
                .path()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string()),
        };
        let new_path = match delta.status() {
            Delta::Deleted => None,
            _ => delta
                .new_file()
                .path()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string()),
        };
        if old_path.is_some() || new_path.is_some() {
            result.push(FileDiff { old_path, new_path });
        }
    }
    Ok(result)
}

/// Reads a single file's content at the given commit, returning None if missing or binary.
pub fn read_file_at(repo: &Repository, commit_sha: &str, path: &str) -> Result<Option<String>> {
    let tree = repo.find_commit(Oid::from_str(commit_sha)?)?.tree()?;
    match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => {
            let blob = repo.find_blob(entry.id())?;
            let raw = blob.content();
            if raw.contains(&0u8) {
                return Ok(None);
            }
            Ok(std::str::from_utf8(raw).ok().map(|s| s.to_string()))
        }
        Err(_) => Ok(None),
    }
}

/// Returns `(folder, folder_depth)` for a file path.
///
/// - `"README.md"`        → `(".", 0)`
/// - `"src/index.ts"`     → `("src", 1)`
/// - `"src/foo/bar.tsx"`  → `("src/foo", 2)`
pub fn path_folder(path: &str) -> (String, i32) {
    match path.rfind('/') {
        Some(idx) => {
            let folder = &path[..idx];
            let depth = folder.split('/').count() as i32;
            (folder.to_string(), depth)
        }
        None => (".".to_string(), 0),
    }
}
