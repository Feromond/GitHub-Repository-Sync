use chrono::{DateTime, Utc};
use git2::Repository;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fs;
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

    let response = request.send().ok()?.json::<GitHubCommit>().ok()?;

    Some(response.sha)
}

fn get_local_commit_sha(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    Some(commit.id().to_string())
}

fn pull_latest_changes(local_path: &str) {
    Command::new("git")
        .arg("-C")
        .arg(local_path)
        .arg("pull")
        .status()
        .expect("Failed to execute git pull");
}

fn load_config() -> Config {
    let config_content = match fs::read_to_string("config.toml") {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Failed to read config.toml: {}", e);
            println!("Press Enter to exit...");
            io::stdout().flush().unwrap();
            let _ = io::stdin().read_line(&mut String::new());
            std::process::exit(1);
        }
    };

    match toml::from_str(&config_content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse config.toml: {}", e);
            println!("Press Enter to exit...");
            io::stdout().flush().unwrap();
            let _ = io::stdin().read_line(&mut String::new());
            std::process::exit(1);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config();

    let check_interval = Duration::from_secs(config.local_repo.check_interval_seconds);
    let mut last_change_time = SystemTime::now();

    loop {
        let repo =
            Repository::open(&config.local_repo.path).expect("Failed to open local repository");

        let latest_remote_commit =
            get_latest_commit_sha(&config.github).expect("Failed to get latest remote commit");
        let local_commit = get_local_commit_sha(&repo).expect("Failed to get local commit");

        if latest_remote_commit != local_commit {
            println!("\nNew changes detected. Pulling updates...");
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
