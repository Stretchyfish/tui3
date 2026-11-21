//! Non-blocking startup check for newer tui3 releases on GitHub.
use serde::Deserialize;
use std::{sync::mpsc, time::Duration};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "Stretchyfish/tui3";
pub const INSTALL_CMD: &str =
    "curl -fsSL https://raw.githubusercontent.com/Stretchyfish/tui3/main/install.sh | sh";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Checking,
    UpToDate,
    Available {
        version: String,
        url: String,
    },
    Unavailable,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('-').next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

pub fn spawn(runtime: &tokio::runtime::Runtime) -> mpsc::Receiver<Status> {
    let (sender, receiver) = mpsc::channel();
    runtime.spawn(async move {
        let status = check_latest().await;
        let _ = sender.send(status);
    });
    receiver
}

async fn check_latest() -> Status {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(format!("tui3/{CURRENT_VERSION}"))
        .build()
    else {
        return Status::Unavailable;
    };
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let Ok(response) = client.get(&url).send().await else {
        return Status::Unavailable;
    };
    if !response.status().is_success() {
        return Status::Unavailable;
    }
    let Ok(release) = response.json::<LatestRelease>().await else {
        return Status::Unavailable;
    };
    let clean = release.tag_name.trim().trim_start_matches('v').to_owned();
    if is_newer(&clean, CURRENT_VERSION) {
        Status::Available {
            version: clean,
            url: if release.html_url.is_empty() {
                format!("https://github.com/{REPO}/releases/latest")
            } else {
                release.html_url
            },
        }
    } else {
        Status::UpToDate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("invalid", "0.1.0"));
    }
}
