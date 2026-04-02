use once_cell::sync::Lazy;
use regex::Regex;

use super::{Analyzer, FileStats};

// export const/let/var/function/class/type/interface/enum NAME
static RE_NAMED_DECL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)^export\s+(?:async\s+)?(?:const|let|var|function\*?|class|type|interface|enum|abstract\s+class)\s+(\w+)",
    )
    .unwrap()
});

// export default function NAME / export default class NAME (named default)
static RE_DEFAULT_NAMED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^export\s+default\s+(?:async\s+)?(?:function\*?|class)\s+(\w+)").unwrap()
});

// Any `export default` — covers both named and anonymous forms for counting
static RE_DEFAULT_ANY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^export\s+default\b").unwrap()
});

// export { ... } or export type { ... } — captures the list contents
static RE_EXPORT_LIST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?ms)^export\s+(?:type\s+)?\{([^}]+)\}").unwrap()
});

pub struct JsAnalyzer;

impl Analyzer for JsAnalyzer {
    fn can_analyze(&self, file_name: &str) -> bool {
        matches!(
            file_extension(file_name),
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
        )
    }

    fn analyze(&self, file_name: &str, content: &str, stats: &mut FileStats) {
        stats.file_type = Some(classify_file_type(file_name).to_string());

        let default_count = RE_DEFAULT_ANY.find_iter(content).count() as u32;

        let named_decl_count = RE_NAMED_DECL.find_iter(content).count() as u32;

        let named_list_count: u32 = RE_EXPORT_LIST
            .captures_iter(content)
            .map(|cap| count_list_items(&cap[1]) as u32)
            .sum();

        let named_total = named_decl_count + named_list_count;

        stats.js_exports_default = Some(default_count);
        stats.js_exports_named = Some(named_total);
        stats.js_exports_total = Some(default_count + named_total);
        stats.js_export_matches_filename = export_matches_stem(file_name, content);
    }
}

fn file_extension(file_name: &str) -> &str {
    file_name.rsplit('.').next().unwrap_or("")
}

/// Returns the stem: everything in the filename (no directory) up to the first dot.
/// `"getFoo.ts"` → `"getFoo"`, `"Button.stories.tsx"` → `"Button"`, `"index.tsx"` → `"index"`
fn file_stem(file_name: &str) -> &str {
    let name = file_name.rsplit('/').next().unwrap_or(file_name);
    name.split('.').next().unwrap_or(name)
}

fn classify_file_type(file_name: &str) -> &'static str {
    let name = file_name.rsplit('/').next().unwrap_or(file_name);

    // Name-based patterns take precedence over directory patterns
    if name.contains(".test.") || name.contains(".spec.") {
        return "test";
    }
    if name.contains(".stories.") || name.contains(".story.") {
        return "story";
    }
    if name.contains(".config.") || is_known_config_file(name) {
        return "config";
    }

    // Directory-based test detection
    if file_name.contains("/__tests__/")
        || file_name.contains("/tests/")
        || file_name.contains("/test/")
        || file_name.starts_with("tests/")
        || file_name.starts_with("test/")
    {
        return "test";
    }

    "source"
}

fn is_known_config_file(name: &str) -> bool {
    matches!(
        name,
        "jest.config.js"
            | "jest.config.ts"
            | "jest.config.mjs"
            | "jest.config.cjs"
            | "vite.config.js"
            | "vite.config.ts"
            | "vite.config.mts"
            | "vitest.config.js"
            | "vitest.config.ts"
            | "vitest.config.mts"
            | "webpack.config.js"
            | "webpack.config.ts"
            | "webpack.config.mjs"
            | "babel.config.js"
            | "babel.config.ts"
            | "babel.config.json"
            | "rollup.config.js"
            | "rollup.config.ts"
            | "rollup.config.mjs"
            | "esbuild.config.js"
            | "esbuild.config.ts"
            | "eslint.config.js"
            | "eslint.config.ts"
            | "eslint.config.mjs"
            | ".eslintrc.js"
            | ".eslintrc.cjs"
            | ".eslintrc.ts"
            | "prettier.config.js"
            | "prettier.config.ts"
            | "prettier.config.mjs"
            | "tailwind.config.js"
            | "tailwind.config.ts"
            | "tailwind.config.mjs"
            | "postcss.config.js"
            | "postcss.config.ts"
            | "next.config.js"
            | "next.config.ts"
            | "next.config.mjs"
            | "nuxt.config.js"
            | "nuxt.config.ts"
            | "svelte.config.js"
    )
}

/// Count non-empty items in an export list, handling trailing commas.
/// `"foo, bar as baz, "` → 2
fn count_list_items(list: &str) -> usize {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .count()
}

/// For `"foo as bar"` returns `"bar"` (the public name); for `"foo"` returns `"foo"`.
fn public_name(item: &str) -> &str {
    item.split_once(" as ")
        .map(|(_, after)| after.trim())
        .unwrap_or(item)
}

/// Returns true if any export's public name matches the file stem (case-insensitive).
///
/// Matched cases:
/// - Named declarations:  `export const getFoo = ...`
/// - Named list exports:  `export { internalName as getFoo }`
/// - Named default:       `export default function getFoo()`
///
/// Anonymous defaults (`export default { ... }`, `export default 42`) never match.
fn export_matches_stem(file_name: &str, content: &str) -> bool {
    let stem = file_stem(file_name).to_lowercase();

    for cap in RE_NAMED_DECL.captures_iter(content) {
        if cap[1].to_lowercase() == stem {
            return true;
        }
    }

    for cap in RE_DEFAULT_NAMED.captures_iter(content) {
        if cap[1].to_lowercase() == stem {
            return true;
        }
    }

    for cap in RE_EXPORT_LIST.captures_iter(content) {
        for item in cap[1].split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if public_name(item).to_lowercase() == stem {
                return true;
            }
        }
    }

    false
}
