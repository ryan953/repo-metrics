use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum Granularity {
    /// Store file, folder, and repo rows
    #[default]
    All,
    /// Store only folder and repo rows (file rows are computed but not saved)
    Folder,
    /// Store only the single repo-wide row per commit
    Repo,
}

#[derive(Parser)]
#[command(name = "repo-metrics", about = "Collect statistics from git repositories")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Analyze a git repository at a given commit and store stats in SQLite
    Analyze(AnalyzeArgs),
    /// Fetch pull request data (authors, reviewers) from the GitHub API
    Prs(PrsArgs),
    /// Show a summary of commits loaded vs missing in the database
    Status(StatusArgs),
}

#[derive(Args)]
pub struct AnalyzeArgs {
    /// Path to the git repository on disk
    pub repo_path: PathBuf,

    /// Repository identity in org/repo format (e.g. "getsentry/sentry")
    #[arg(long)]
    pub repo: String,

    /// Tip of the commit walk: branch name, tag, or SHA (all ancestors are analyzed)
    #[arg(long, default_value = "HEAD")]
    pub commit: String,

    /// Oldest commit date to include, in YYYY-MM-DD format (e.g. 2022-01-01).
    /// Commits older than this date are skipped. Omit to walk the full history.
    #[arg(long)]
    pub since: Option<String>,

    /// Path to the SQLite database file (created if it doesn't exist)
    #[arg(long, default_value = "repo-metrics.db")]
    pub db: PathBuf,

    /// How much detail to store per commit.
    /// Note: file-level analysis always runs; this only controls what gets written to the database.
    #[arg(long, default_value = "all")]
    pub granularity: Granularity,
}

#[derive(Args)]
pub struct StatusArgs {
    /// Path to the git repository on disk
    pub repo_path: PathBuf,

    /// Repository identity in org/repo format (e.g. "getsentry/sentry")
    #[arg(long)]
    pub repo: String,

    /// Tip of the commit walk: branch name, tag, or SHA (all ancestors are analyzed)
    #[arg(long, default_value = "HEAD")]
    pub commit: String,

    /// Oldest commit date to include, in YYYY-MM-DD format (e.g. 2022-01-01).
    #[arg(long)]
    pub since: Option<String>,

    /// Path to the SQLite database file
    #[arg(long, default_value = "repo-metrics.db")]
    pub db: PathBuf,
}

#[derive(Args)]
pub struct PrsArgs {
    /// Repository identity in org/repo format (e.g. "getsentry/sentry")
    #[arg(long)]
    pub repo: String,

    /// GitHub personal access token (or set GITHUB_TOKEN env var)
    #[arg(long, env = "GITHUB_TOKEN")]
    pub token: String,

    /// Path to the SQLite database file (created if it doesn't exist)
    #[arg(long, default_value = "repo-metrics.db")]
    pub db: PathBuf,

    /// Only fetch PRs created on or after this date (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,
}
