use std::collections::HashMap;

use crate::db::store::StatRow;

/// Groups file-level rows by their folder and sums all stats.
/// Each resulting row has `file_name = None` and represents one folder.
pub fn aggregate_to_folders(file_rows: &[StatRow]) -> Vec<StatRow> {
    let mut map: HashMap<String, StatRow> = HashMap::new();

    for row in file_rows {
        let agg = map.entry(row.folder.clone()).or_insert_with(|| StatRow {
            repo: row.repo.clone(),
            commit_sha: row.commit_sha.clone(),
            commit_date: row.commit_date.clone(),
            row_type: "folder".to_string(),
            folder: row.folder.clone(),
            folder_depth: row.folder_depth,
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

        agg.file_count += row.file_count;
        agg.sloc_nonblank += row.sloc_nonblank;
        agg.sloc_noncomment += row.sloc_noncomment;
        agg.source_file_count += row.source_file_count;
        agg.test_file_count += row.test_file_count;
        agg.story_file_count += row.story_file_count;
        agg.config_file_count += row.config_file_count;
        add_opt(&mut agg.js_exports_default, row.js_exports_default);
        add_opt(&mut agg.js_exports_named, row.js_exports_named);
        add_opt(&mut agg.js_exports_total, row.js_exports_total);
        agg.js_export_matches_filename += row.js_export_matches_filename;
        agg.py_file_count += row.py_file_count;
        agg.js_file_count += row.js_file_count;
        agg.jsx_file_count += row.jsx_file_count;
        agg.ts_file_count += row.ts_file_count;
        agg.tsx_file_count += row.tsx_file_count;
        agg.css_file_count += row.css_file_count;
        agg.html_file_count += row.html_file_count;
        agg.md_file_count += row.md_file_count;
        agg.json_file_count += row.json_file_count;
        agg.yaml_file_count += row.yaml_file_count;
    }

    map.into_values().collect()
}

/// Aggregates all file-level rows into a single repo-wide row (`folder = "."`, `file_name = None`).
pub fn aggregate_to_repo(
    repo: &str,
    commit_sha: &str,
    commit_date: &str,
    file_rows: &[StatRow],
) -> StatRow {
    let mut result = StatRow {
        repo: repo.to_string(),
        commit_sha: commit_sha.to_string(),
        commit_date: commit_date.to_string(),
        row_type: "repo".to_string(),
        folder: ".".to_string(),
        folder_depth: 0,
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
    };

    for row in file_rows {
        result.file_count += row.file_count;
        result.sloc_nonblank += row.sloc_nonblank;
        result.sloc_noncomment += row.sloc_noncomment;
        result.source_file_count += row.source_file_count;
        result.test_file_count += row.test_file_count;
        result.story_file_count += row.story_file_count;
        result.config_file_count += row.config_file_count;
        add_opt(&mut result.js_exports_default, row.js_exports_default);
        add_opt(&mut result.js_exports_named, row.js_exports_named);
        add_opt(&mut result.js_exports_total, row.js_exports_total);
        result.js_export_matches_filename += row.js_export_matches_filename;
        result.py_file_count += row.py_file_count;
        result.js_file_count += row.js_file_count;
        result.jsx_file_count += row.jsx_file_count;
        result.ts_file_count += row.ts_file_count;
        result.tsx_file_count += row.tsx_file_count;
        result.css_file_count += row.css_file_count;
        result.html_file_count += row.html_file_count;
        result.md_file_count += row.md_file_count;
        result.json_file_count += row.json_file_count;
        result.yaml_file_count += row.yaml_file_count;
    }

    result
}

/// Applies an arithmetic delta to an aggregate row (folder or repo).
///
/// Subtracts the stats from `removed` file rows and adds the stats from `added` file rows.
/// Updates `commit_sha` and `commit_date` on the returned row.
pub fn apply_delta(
    mut base: StatRow,
    commit_sha: &str,
    commit_date: &str,
    removed: &[StatRow],
    added: &[StatRow],
) -> StatRow {
    base.commit_sha = commit_sha.to_string();
    base.commit_date = commit_date.to_string();
    for r in removed {
        base.file_count -= r.file_count;
        base.sloc_nonblank -= r.sloc_nonblank;
        base.sloc_noncomment -= r.sloc_noncomment;
        base.source_file_count -= r.source_file_count;
        base.test_file_count -= r.test_file_count;
        base.story_file_count -= r.story_file_count;
        base.config_file_count -= r.config_file_count;
        sub_opt(&mut base.js_exports_default, r.js_exports_default);
        sub_opt(&mut base.js_exports_named, r.js_exports_named);
        sub_opt(&mut base.js_exports_total, r.js_exports_total);
        base.js_export_matches_filename -= r.js_export_matches_filename;
        base.py_file_count -= r.py_file_count;
        base.js_file_count -= r.js_file_count;
        base.jsx_file_count -= r.jsx_file_count;
        base.ts_file_count -= r.ts_file_count;
        base.tsx_file_count -= r.tsx_file_count;
        base.css_file_count -= r.css_file_count;
        base.html_file_count -= r.html_file_count;
        base.md_file_count -= r.md_file_count;
        base.json_file_count -= r.json_file_count;
        base.yaml_file_count -= r.yaml_file_count;
    }
    for a in added {
        base.file_count += a.file_count;
        base.sloc_nonblank += a.sloc_nonblank;
        base.sloc_noncomment += a.sloc_noncomment;
        base.source_file_count += a.source_file_count;
        base.test_file_count += a.test_file_count;
        base.story_file_count += a.story_file_count;
        base.config_file_count += a.config_file_count;
        add_opt(&mut base.js_exports_default, a.js_exports_default);
        add_opt(&mut base.js_exports_named, a.js_exports_named);
        add_opt(&mut base.js_exports_total, a.js_exports_total);
        base.js_export_matches_filename += a.js_export_matches_filename;
        base.py_file_count += a.py_file_count;
        base.js_file_count += a.js_file_count;
        base.jsx_file_count += a.jsx_file_count;
        base.ts_file_count += a.ts_file_count;
        base.tsx_file_count += a.tsx_file_count;
        base.css_file_count += a.css_file_count;
        base.html_file_count += a.html_file_count;
        base.md_file_count += a.md_file_count;
        base.json_file_count += a.json_file_count;
        base.yaml_file_count += a.yaml_file_count;
    }
    base
}

/// Adds `src` into `dst`, treating `None` as absent (not as zero).
/// Once any JS file contributes a value, the field becomes `Some`.
fn add_opt(dst: &mut Option<i64>, src: Option<i64>) {
    if let Some(v) = src {
        *dst = Some(dst.unwrap_or(0) + v);
    }
}

fn sub_opt(dst: &mut Option<i64>, src: Option<i64>) {
    if let Some(v) = src {
        *dst = Some(dst.unwrap_or(0) - v);
    }
}
