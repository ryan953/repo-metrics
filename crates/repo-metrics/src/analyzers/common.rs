use super::{Analyzer, FileStats};

pub struct CommonAnalyzer;

impl Analyzer for CommonAnalyzer {
    fn can_analyze(&self, _file_name: &str) -> bool {
        true
    }

    fn analyze(&self, _file_name: &str, content: &str, stats: &mut FileStats) {
        let mut nonblank = 0u32;
        let mut noncomment = 0u32;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            nonblank += 1;
            if !is_comment_line(trimmed) {
                noncomment += 1;
            }
        }

        stats.sloc_nonblank = nonblank;
        stats.sloc_noncomment = noncomment;
    }
}

/// Heuristic check for common single-line comment styles across languages.
/// Handles JS/TS/Rust/Go (`//`), Python/Shell/YAML (`#`),
/// block comment lines (`*`, `/*`, `*/`), HTML (`<!--`, `-->`), SQL/Lua (`--`).
fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("-->")
        || trimmed.starts_with("--")
}
