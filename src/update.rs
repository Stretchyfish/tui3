//! Non-blocking release check and self-update worker for tui3 on GitHub.
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "Stretchyfish/tui3";

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

pub enum Command {
    Check,
    Update { version: String },
}

pub enum Update {
    Checked(Status),
    Progress(crate::app::ServerUpdateStatus),
}

pub struct Backend {
    pub commands: tokio::sync::mpsc::Sender<Command>,
    pub updates: mpsc::Receiver<Update>,
}

pub fn spawn(runtime: &tokio::runtime::Runtime) -> Backend {
    let (sender, receiver) = mpsc::channel();
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<Command>(8);
    runtime.spawn(async move {
        let _ = sender.send(Update::Checked(check_latest().await));
        while let Some(command) = cmd_rx.recv().await {
            match command {
                Command::Check => {
                    let _ = sender.send(Update::Checked(check_latest().await));
                }
                Command::Update { version } => {
                    install_release(&version, &sender).await;
                }
            }
        }
    });
    Backend {
        commands: cmd_tx,
        updates: receiver,
    }
}

fn asset_name() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        other => return Err(format!("Unsupported OS for self-update: {other}")),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => return Err(format!("Unsupported architecture for self-update: {other}")),
    };
    Ok(format!("tui3-{os}-{arch}.tar.gz"))
}

fn install_target() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(name) = exe.file_name().and_then(|n| n.to_str())
        && name == "tui3"
        && !exe.to_string_lossy().contains("/target/")
        && exe.parent().is_some_and(|dir| dir.exists())
    {
        return Ok(exe);
    }
    let home = std::env::var_os("HOME").ok_or("Cannot determine HOME for tui3 install.")?;
    Ok(PathBuf::from(home).join(".local/bin/tui3"))
}

async fn install_release(version: &str, sender: &mpsc::Sender<Update>) {
    use crate::app::ServerUpdateStatus;
    let target_version = version.trim_start_matches('v').to_owned();
    let _ = sender.send(Update::Progress(ServerUpdateStatus::Downloading {
        target_version: target_version.clone(),
        tick: 20,
    }));
    let asset = match asset_name() {
        Ok(asset) => asset,
        Err(error) => {
            let _ = sender.send(Update::Progress(ServerUpdateStatus::Failed(error)));
            return;
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("tui3/{CURRENT_VERSION}"))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let _ = sender.send(Update::Progress(ServerUpdateStatus::Failed(
                error.to_string(),
            )));
            return;
        }
    };
    let urls = [
        format!("https://github.com/{REPO}/releases/download/v{target_version}/{asset}"),
        format!("https://github.com/{REPO}/releases/download/{target_version}/{asset}"),
    ];
    let mut archive_bytes = None;
    let mut last_error = String::new();
    for url in &urls {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    archive_bytes = Some(bytes);
                    break;
                }
                Err(error) => last_error = error.to_string(),
            },
            Ok(resp) => last_error = format!("Download failed with HTTP {}", resp.status()),
            Err(error) => last_error = error.to_string(),
        }
    }
    let Some(bytes) = archive_bytes else {
        let _ = sender.send(Update::Progress(ServerUpdateStatus::Failed(last_error)));
        return;
    };
    let _ = sender.send(Update::Progress(ServerUpdateStatus::Installing {
        target_version: target_version.clone(),
    }));
    let install_result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let archive_path = tmp.path().join(&asset);
        fs::write(&archive_path, &bytes).map_err(|e| e.to_string())?;
        let status = std::process::Command::new("tar")
            .args([
                "-xzf",
                &archive_path.to_string_lossy(),
                "-C",
                &tmp.path().to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run tar: {e}"))?;
        if !status.success() {
            return Err("Failed to extract tui3 release archive.".into());
        }
        let extracted = tmp.path().join("tui3");
        if !extracted.is_file() {
            return Err("Release archive did not contain a tui3 binary.".into());
        }
        let target = install_target()?;
        install_binary(&extracted, &target)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|res| res);

    match install_result {
        Ok(()) => {
            let _ = sender.send(Update::Progress(ServerUpdateStatus::Completed {
                target_version,
            }));
        }
        Err(error) => {
            let _ = sender.send(Update::Progress(ServerUpdateStatus::Failed(error)));
        }
    }
}

fn install_binary(source: &Path, target: &Path) -> Result<(), String> {
    let parent = target.parent().ok_or("Invalid tui3 install path.")?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("Failed to stage binary in {}: {e}", parent.display()))?;
    let data = fs::read(source).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut staged, &data).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    staged
        .persist(target)
        .map_err(|e| format!("Failed to install {}: {}", target.display(), e.error))?;
    Ok(())
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
