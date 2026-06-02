pub mod common;
pub mod javascript;
pub mod python;

/// Stats collected for a single file. Populated by one or more `Analyzer`s.
#[derive(Default)]
pub struct FileStats {
    /// Lines that are non-empty/non-whitespace
    pub sloc_nonblank: u32,
    /// Non-blank lines that are also not comment lines
    pub sloc_noncomment: u32,

    /// JS/TS only: "source" | "test" | "story" | "config". None for all other files.
    pub file_type: Option<String>,

    /// JS/TS only: counts of each export kind. None for non-JS files.
    pub js_exports_default: Option<u32>,
    pub js_exports_named: Option<u32>,
    pub js_exports_total: Option<u32>,

    /// JS/TS only: true if any export's public name matches the file stem
    /// (case-insensitive, stem = filename up to first dot).
    pub js_export_matches_filename: bool,
}

pub trait Analyzer: Send + Sync {
    /// Whether this analyzer handles the given file (by path/extension).
    fn can_analyze(&self, file_name: &str) -> bool;

    /// Populate `stats` for the given file. Called only when `can_analyze` returns true.
    /// Multiple analyzers may run on the same file; each writes its own fields.
    fn analyze(&self, file_name: &str, content: &str, stats: &mut FileStats);
}
