use super::{Analyzer, FileStats};

pub struct PyAnalyzer;

impl Analyzer for PyAnalyzer {
    fn can_analyze(&self, file_name: &str) -> bool {
        file_name.ends_with(".py")
    }

    fn analyze(&self, file_name: &str, _content: &str, stats: &mut FileStats) {
        stats.file_type = Some(classify_file_type(file_name).to_string());
    }
}

fn classify_file_type(file_name: &str) -> &'static str {
    let name = file_name.rsplit('/').next().unwrap_or(file_name);

    // test_foo.py — pytest convention
    if name.starts_with("test_") {
        return "test";
    }
    // foo_test.py — Go-style, also used in some Python projects
    if name.ends_with("_test.py") {
        return "test";
    }

    // Directory-based detection (conftest.py lives in test dirs, but is config-ish;
    // files under tests/ or test/ are tests by convention)
    if file_name.contains("/tests/")
        || file_name.contains("/test/")
        || file_name.starts_with("tests/")
        || file_name.starts_with("test/")
    {
        return "test";
    }

    "source"
}
