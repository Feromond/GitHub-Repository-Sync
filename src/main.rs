use chrono::{DateTime, Utc};
use log::{error, info};
use reqwest::Client;
use serde::Deserialize;
use serde_json;
use simplelog::*;
use std::fs::File;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

#[derive(Deserialize)]
struct AppConfig {
    repo_path: String,
    owner: String,
    repo: String,
    target_branch: String,
    pat: String,
    check_interval_seconds: u64,
}

#[derive(Deserialize)]
struct GitHubCommit {
    sha: String,
}

// Read the config file and parse it into the AppConfig struct
fn read_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new("config.toml");

    if !config_path.exists() {
        error!("Config file not found.");
        eprintln!("Config file not found in the same directory. Please ensure 'config.toml' is present.");
        // Prompt user to press Enter before exiting
        print!("Press Enter to exit...");
        io::stdout().flush()?;
        let _ = io::stdin().read_line(&mut String::new());
        std::process::exit(1);
    }

    let config_content = fs::read_to_string(config_path)?;
    let config: AppConfig = toml::from_str(&config_content)?;
    info!("Config file read successfully.");
    Ok(config)
}

// Get the latest commit SHA from the GitHub repository's target branch
async fn get_latest_commit(config: &AppConfig) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::new();
    // The GitHub Commits API returns a list of commits in reverse chronological order
    // so the first item is the latest commit
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/commits?sha={}",
        config.owner, config.repo, config.target_branch
    );

    let response = client
        .get(api_url)
        .header("User-Agent", "github-sync-tool")
        .bearer_auth(&config.pat)
        .send()
        .await?;

    if !response.status().is_success() {
        let status_code = response.status();
        let resp_text = response.text().await?;
        error!("GitHub API returned error {}: {}", status_code, resp_text);
        return Err("Failed to get latest commit from GitHub".into());
    }

    let commits: Vec<GitHubCommit> = serde_json::from_str(&response.text().await?)?;
    if commits.is_empty() {
        return Err("No commits found in the repository for the specified branch.".into());
    }

    let latest = commits[0].sha.clone();
    info!("Received latest commit from remote: {}", latest);
    Ok(latest)
}

// Get the local commit (HEAD) SHA
fn get_local_commit(repo_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?;

    let commit_id = String::from_utf8(output.stdout)?.trim().to_string();
    info!("Local commit ID: {}", commit_id);

    Ok(commit_id)
}

// Pull changes from GitHub
fn pull_changes(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = &config.repo_path;

    // Construct GitHub URL with credentials
    let url_with_credentials = format!(
        "https://x-access-token:{}@github.com/{}/{}.git",
        config.pat, config.owner, config.repo
    );

    // 1. Fetch
    let fetch_refspec = "+refs/heads/*:refs/remotes/origin/*";
    let status_fetch = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("fetch")
        .arg("--prune")
        .arg(&url_with_credentials)
        .arg(&fetch_refspec)
        .status()?;

    if !status_fetch.success() {
        // Capture output if fetch fails
        let output_fetch = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("fetch")
            .arg("--prune")
            .arg(&url_with_credentials)
            .arg(&fetch_refspec)
            .output()?;

        let stdout = String::from_utf8_lossy(&output_fetch.stdout);
        let stderr = String::from_utf8_lossy(&output_fetch.stderr);
        error!(
            "Failed to fetch from remote. stdout: {}, stderr: {}",
            stdout, stderr
        );
        return Err("Failed to fetch from remote".into());
    } else {
        info!("Fetched all branches from remote.");
    }

    // 2. Check if the target branch exists locally
    let status_branch_check = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg(&config.target_branch)
        .status()?;

    if !status_branch_check.success() {
        // 2a. Branch doesn't exist locally; create it to track remote branch
        let remote_branch = format!("origin/{}", &config.target_branch);
        let status_checkout_new = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("checkout")
            .arg("-b")
            .arg(&config.target_branch)
            .arg("--track")
            .arg(&remote_branch)
            .status()?;

        if !status_checkout_new.success() {
            let output_checkout_new = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .arg("checkout")
                .arg("-b")
                .arg(&config.target_branch)
                .arg("--track")
                .arg(&remote_branch)
                .output()?;

            let stdout_new = String::from_utf8_lossy(&output_checkout_new.stdout);
            let stderr_new = String::from_utf8_lossy(&output_checkout_new.stderr);
            error!(
                "Failed to create and checkout branch '{}'. stdout: {}, stderr: {}",
                config.target_branch, stdout_new, stderr_new
            );
            return Err("Failed to create and checkout branch".into());
        } else {
            info!("Created and checked out branch '{}'", config.target_branch);
        }
    } else {
        // 2b. Branch exists locally; just check it out
        let status_checkout = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("checkout")
            .arg(&config.target_branch)
            .status()?;

        if !status_checkout.success() {
            let output_checkout = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .arg("checkout")
                .arg(&config.target_branch)
                .output()?;

            let stdout = String::from_utf8_lossy(&output_checkout.stdout);
            let stderr = String::from_utf8_lossy(&output_checkout.stderr);
            error!(
                "Failed to checkout branch '{}'. stdout: {}, stderr: {}",
                config.target_branch, stdout, stderr
            );
            return Err("Failed to checkout branch".into());
        } else {
            info!("Checked out branch '{}'", config.target_branch);
        }
    }

    // 3. Pull
    let status_pull = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("pull")
        .arg(&url_with_credentials)
        .arg(&config.target_branch)
        .status()?;

    if !status_pull.success() {
        let output_pull = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("pull")
            .arg(&url_with_credentials)
            .arg(&config.target_branch)
            .output()?;

        let stdout = String::from_utf8_lossy(&output_pull.stdout);
        let stderr = String::from_utf8_lossy(&output_pull.stderr);
        error!(
            "Failed to pull changes. stdout: {}, stderr: {}",
            stdout, stderr
        );
        return Err("Failed to pull changes".into());
    } else {
        info!("Changes pulled successfully.");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to a file
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        simplelog::Config::default(),
        File::create("app.log").unwrap(),
    )])?;

    info!("Starting GitHub repo sync application");

    let config: AppConfig = read_config()?;
    let mut last_change_time = SystemTime::now();

    // Main loop: check remote vs local commit every `check_interval_seconds`
    loop {
        match get_latest_commit(&config).await {
            Ok(remote_commit) => match get_local_commit(&config.repo_path) {
                Ok(local_commit) => {
                    if remote_commit != local_commit {
                        info!("New changes detected. Pulling updates...");
                        if let Err(e) = pull_changes(&config) {
                            error!("Failed to pull changes: {}", e);
                        } else {
                            last_change_time = SystemTime::now();
                        }
                    } else {
                        let elapsed = last_change_time.elapsed()?.as_secs();
                        let last_change_time_utc: DateTime<Utc> = last_change_time.into();
                        let formatted_time = last_change_time_utc.format("%Y-%m-%d %H:%M:%S");
                        print!(
                            "\rNo new changes since {}. Elapsed time: {} seconds.",
                            formatted_time, elapsed
                        );
                        io::stdout().flush()?;
                    }
                }
                Err(e) => {
                    error!("Failed to get local commit: {}", e);
                }
            },
            Err(e) => {
                error!("Failed to get latest commit from GitHub: {}", e);
            }
        }

        sleep(Duration::from_secs(config.check_interval_seconds)).await;
    }
}
