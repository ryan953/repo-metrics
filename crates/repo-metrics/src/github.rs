use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub struct GitHubClient {
    client: reqwest::blocking::Client,
    token: String,
    repo: String,
}

#[derive(Deserialize, Debug)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub user: User,
    pub state: String,
    pub draft: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub closed_at: Option<String>,
    pub merged: Option<bool>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_files: Option<i64>,
    pub base: GitRef,
    pub head: GitRef,
}

#[derive(Deserialize, Debug)]
pub struct User {
    pub login: String,
}

#[derive(Deserialize, Debug)]
pub struct GitRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
}

#[derive(Deserialize, Debug)]
pub struct Review {
    pub id: i64,
    pub user: Option<User>,
    pub state: String,
    pub submitted_at: Option<String>,
}

impl GitHubClient {
    pub fn new(token: &str, repo: &str) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("repo-metrics/0.1")
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            client,
            token: token.to_string(),
            repo: repo.to_string(),
        })
    }

    fn get(&self, url: &str) -> Result<reqwest::blocking::Response> {
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .with_context(|| format!("request to {url} failed"))?;

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
                if remaining.to_str().unwrap_or("1") == "0" {
                    if let Some(reset) = resp.headers().get("x-ratelimit-reset") {
                        let reset_ts: i64 = reset.to_str().unwrap_or("0").parse().unwrap_or(0);
                        let now = chrono::Utc::now().timestamp();
                        let wait = (reset_ts - now).max(0);
                        bail!(
                            "GitHub API rate limit exceeded. Resets in {}s ({}m).",
                            wait,
                            wait / 60
                        );
                    }
                }
            }
        }

        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            bail!("GitHub API error {status}: {body}");
        }

        Ok(resp)
    }

    pub fn rate_limit_remaining(&self) -> Result<(u32, u32)> {
        let resp = self
            .client
            .get("https://api.github.com/rate_limit")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send()?;
        let remaining: u32 = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let limit: u32 = resp
            .headers()
            .get("x-ratelimit-limit")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok((remaining, limit))
    }

    /// Fetch PRs sorted by created date descending, stopping at `since` date.
    pub fn list_pulls(&self, since: Option<&str>) -> Result<Vec<PullRequest>> {
        let mut all = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "https://api.github.com/repos/{}/pulls?state=all&sort=created&direction=desc&per_page=100&page={}",
                self.repo, page
            );
            let resp = self.get(&url)?;
            let pulls: Vec<PullRequest> = resp.json().context("failed to parse pulls response")?;

            if pulls.is_empty() {
                break;
            }

            let mut hit_cutoff = false;
            for pr in pulls {
                if let Some(since) = since {
                    if pr.created_at.len() >= 10 && &pr.created_at[..10] < since {
                        hit_cutoff = true;
                        break;
                    }
                }
                all.push(pr);
            }

            if hit_cutoff {
                break;
            }

            page += 1;
        }

        Ok(all)
    }

    /// Fetch full details for a single PR (includes additions/deletions/changed_files).
    pub fn get_pull(&self, pr_number: i64) -> Result<PullRequest> {
        let url = format!(
            "https://api.github.com/repos/{}/pulls/{}",
            self.repo, pr_number
        );
        let resp = self.get(&url)?;
        resp.json()
            .context("failed to parse pull request detail response")
    }

    pub fn list_reviews(&self, pr_number: i64) -> Result<Vec<Review>> {
        let mut all = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "https://api.github.com/repos/{}/pulls/{}/reviews?per_page=100&page={}",
                self.repo, pr_number, page
            );
            let resp = self.get(&url)?;
            let reviews: Vec<Review> = resp.json().context("failed to parse reviews response")?;

            if reviews.is_empty() {
                break;
            }
            all.extend(reviews);
            page += 1;
        }

        Ok(all)
    }
}
