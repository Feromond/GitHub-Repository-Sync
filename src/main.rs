use chrono::{DateTime, Utc};
use git2::Repository;
use log::{error, info};
use reqwest::blocking::Client;
use serde::Deserialize;
use simplelog::*;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Deserialize)]
struct Config {
    github: GitHubConfig,
    local_repo: LocalRepoConfig,
}

#[derive(Deserialize)]
struct GitHubConfig {
    owner: String,
    repo: String,
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

fn get_latest_commit_sha(config: &GitHubConfig) -> Option<String> {
    let url = format!(
        "{}/{}/{}/commits/main",
        GITHUB_API_URL, config.owner, config.repo
    );
    let client = Client::new();

    let mut request = client.get(&url).header("User-Agent", "rust-script");

    if let Some(token) = &config.access_token {
        request = request.header("Authorization", format!("token {}", token));
    }

    let response = request.send().ok()?;
    let commit: GitHubCommit = response.json().ok()?;

    info!("Fetched latest remote commit: {}", commit.sha);
    Some(commit.sha)
}

fn get_local_commit_sha(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    let local_commit = commit.id().to_string();
    info!("Fetched local commit: {}", local_commit);
    Some(local_commit)
}

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        ConfigBuilder::new().build(),
        File::create("app.log").unwrap(),
    )])?;

    info!("Starting application");

    let config = load_config();

    let check_interval = Duration::from_secs(config.local_repo.check_interval_seconds);
    let mut last_change_time = SystemTime::now();

    loop {
        let repo = match Repository::open(&config.local_repo.path) {
            Ok(repo) => repo,
            Err(e) => {
                error!("Failed to open local repository: {}", e);
                continue;
            }
        };

        let latest_remote_commit = match get_latest_commit_sha(&config.github) {
            Some(commit) => commit,
            None => {
                error!("Failed to get latest remote commit.");
                continue;
            }
        };

        let local_commit = match get_local_commit_sha(&repo) {
            Some(commit) => commit,
            None => {
                error!("Failed to get local commit.");
                continue;
            }
        };

        if latest_remote_commit != local_commit {
            info!("New changes detected. Pulling updates...");
            pull_latest_changes(&config.local_repo.path);
            last_change_time = SystemTime::now();
        } else {
            let elapsed = last_change_time.elapsed()?.as_secs();
            let last_change_time: DateTime<Utc> = last_change_time.into();
            let formatted_time = last_change_time.format("%Y-%m-%d %H:%M:%S");
            print!(
                "\rNo new changes since {} UTC. Elapsed time: {} seconds.",
                formatted_time, elapsed
            );
            io::stdout().flush()?; // Ensure the output is flushed
        }

        thread::sleep(check_interval);
    }
}
