use chrono::{DateTime, Utc};
use git2::Repository;
use log::{error, info};
use reqwest::Client;
use serde::Deserialize;
use simplelog::*;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::process::Command;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

#[derive(Deserialize)]
struct Config {
    github: GitHubConfig,
    local_repo: LocalRepoConfig,
}

#[derive(Deserialize)]
struct GitHubConfig {
    owner: String,
    repo: String,
    target_branch: String,
    access_token: Option<String>,
}

#[derive(Deserialize)]
struct LocalRepoConfig {
    path: String,
    check_interval_seconds: u64,
}

const GITHUB_API_URL: &str = "https://api.github.com/repos";

#[derive(Deserialize)]
struct GitHubCommit {
    sha: String,
}

// Utility function for formatting the time in a consistent format.
fn format_time(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

// Exponential backoff function to avoid hammering GitHub with too many requests in case of errors.
fn exponential_backoff(attempt: u32) -> Duration {
    let delay = 2u64.pow(attempt.min(6)); // Cap the delay to 64 seconds (2^6)
    Duration::from_secs(delay)
}

// Fetch the latest commit SHA from GitHub asynchronously using reqwest.
async fn get_latest_commit_sha(config: &GitHubConfig) -> Option<String> {
    let url = format!(
        "{}/{}/{}/commits/{}",
        GITHUB_API_URL, config.owner, config.repo, config.target_branch
    );
    let client = Client::new();

    let mut request = client.get(&url).header("User-Agent", "rust-script");

    if let Some(token) = &config.access_token {
        request = request.header("Authorization", format!("token {}", token));
    }

    match request.send().await {
        Ok(response) => match response.json::<GitHubCommit>().await {
            Ok(commit) => {
                info!("Fetched latest remote commit: {}", commit.sha);
                Some(commit.sha)
            }
            Err(e) => {
                error!("Failed to parse commit response: {}", e);
                None
            }
        },
        Err(e) => {
            error!("Failed to send request: {}", e);
            None
        }
    }
}

// Get the local commit SHA from the local Git repository.
fn get_local_commit_sha(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    let local_commit = commit.id().to_string();
    info!("Fetched local commit: {}", local_commit);
    Some(local_commit)
}

// Pull the latest changes from the remote repository.
fn pull_latest_changes(local_path: &str) {
    info!("Pulling latest changes...");
    let status = Command::new("git")
        .arg("-C")
        .arg(local_path)
        .arg("pull")
        .status();

    match status {
        Ok(status) if status.success() => info!("Successfully pulled latest changes."),
        Ok(_) => error!("Failed to pull latest changes: Git command did not succeed."),
        Err(e) => error!("Failed to execute git pull: {}", e),
    }
}

// Load the configuration from the config.toml file.
fn load_config() -> Config {
    let config_content = match fs::read_to_string("config.toml") {
        Ok(content) => {
            info!("Config file read successfully.");
            content
        }
        Err(e) => {
            error!("Failed to read config.toml: {}", e);
            println!("Press Enter to exit...");
            io::stdout().flush().unwrap();
            let _ = io::stdin().read_line(&mut String::new());
            std::process::exit(1);
        }
    };

    match toml::from_str(&config_content) {
        Ok(config) => {
            info!("Config file parsed successfully.");
            config
        }
        Err(e) => {
            error!("Failed to parse config.toml: {}", e);
            println!("Press Enter to exit...");
            io::stdout().flush().unwrap();
            let _ = io::stdin().read_line(&mut String::new());
            std::process::exit(1);
        }
    }
}

// Main async function with exponential backoff and time formatting.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        ConfigBuilder::new().build(),
        File::create("app.log").unwrap(),
    )])?;

    info!("Starting application");

    // Load config
    let config = load_config();

    let check_interval = Duration::from_secs(config.local_repo.check_interval_seconds);
    let mut last_change_time = SystemTime::now();

    let mut backoff_attempt = 0;

    // Main loop for checking repository status
    loop {
        let repo = match Repository::open(&config.local_repo.path) {
            Ok(repo) => repo,
            Err(e) => {
                error!("Failed to open local repository: {}", e);
                sleep(check_interval).await;
                continue;
            }
        };

        let latest_remote_commit = match get_latest_commit_sha(&config.github).await {
            Some(commit) => commit,
            None => {
                error!("Failed to get latest remote commit.");
                sleep(exponential_backoff(backoff_attempt)).await;
                backoff_attempt += 1;
                continue;
            }
        };

        let local_commit = match get_local_commit_sha(&repo) {
            Some(commit) => commit,
            None => {
                error!("Failed to get local commit.");
                sleep(exponential_backoff(backoff_attempt)).await;
                backoff_attempt += 1;
                continue;
            }
        };

        // If new changes are detected, pull the latest changes
        if latest_remote_commit != local_commit {
            info!("New changes detected. Pulling updates...");
            pull_latest_changes(&config.local_repo.path);
            last_change_time = SystemTime::now();
            backoff_attempt = 0; // Reset backoff after successful operation
        } else {
            let elapsed = last_change_time.elapsed()?.as_secs();
            let formatted_time = format_time(last_change_time);
            print!(
                "\rNo new changes since {} UTC. Elapsed time: {} seconds.",
                formatted_time, elapsed
            );
            io::stdout().flush()?; // Ensure the output is flushed
        }

        // Sleep for the configured interval before the next check
        sleep(check_interval).await;
    }
}
