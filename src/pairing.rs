//! Local pairing uses T3's supported CLI, never the desktop's private session.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct SavedSession {
    origin: String,
    environment_id: String,
    access_token: String,
}

pub fn session_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("Cannot locate config directory for saving the pairing session.")?;
    Ok(root.join("tui3/session.json"))
}

pub fn load(path: &Path, origin: &Url, environment_id: &str) -> Option<String> {
    let saved: SavedSession = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (saved.origin == origin.as_str() && saved.environment_id == environment_id)
        .then_some(saved.access_token)
}

pub fn save(path: &Path, origin: &Url, environment_id: &str, token: &str) -> Result<(), String> {
    let directory = path.parent().ok_or("Invalid session storage path.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|_| "Could not create private session directory.")?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Could not protect session directory.")?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(directory).map_err(|_| "Could not create session directory.")?;
    let data = serde_json::to_vec(&SavedSession {
        origin: origin.as_str().into(),
        environment_id: environment_id.into(),
        access_token: token.into(),
    })
    .map_err(|_| "Could not encode session.")?;
    // NamedTempFile creates a private file, and rename avoids partial credentials on disk.
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "Could not create private session file.")?;
    file.write_all(&data)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "Could not save session.")?;
    file.persist(path).map_err(|_| "Could not save session.")?;
    Ok(())
}

pub fn local_state_dir() -> Option<PathBuf> {
    std::env::var_os("T3_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".t3/userdata")))
}

fn validate_target(state: &Path, origin: &Url) -> Result<PathBuf, String> {
    #[derive(Deserialize)]
    struct Runtime {
        origin: String,
    }
    let data = fs::read(state.join("server-runtime.json"))
        .map_err(|_| "Local T3 runtime not found. Start T3 Code before pairing.")?;
    let runtime: Runtime =
        serde_json::from_slice(&data).map_err(|_| "Invalid local T3 runtime file.")?;
    let local_origin = Url::parse(&runtime.origin).map_err(|_| "Invalid local T3 origin.")?;
    if &local_origin != origin
        || !matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        return Err("Automatic pairing supports the local T3 server only. Remote sign-in is not available yet.".into());
    }
    if state.file_name().and_then(|name| name.to_str()) != Some("userdata") {
        return Err("Local pairing requires T3_STATE_DIR to end in userdata.".into());
    }
    Ok(state
        .parent()
        .ok_or("Invalid T3 state directory.")?
        .to_owned())
}

fn parse_credential(output: &[u8]) -> Result<String, String> {
    std::str::from_utf8(output)
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.strip_prefix("Token: ")))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            "T3 CLI returned no pairing credential. Check that your T3 CLI is up to date.".into()
        })
}

pub async fn local_credential(origin: &Url) -> Result<String, String> {
    let state = local_state_dir().ok_or("Cannot find local T3 state directory.")?;
    let base = validate_target(&state, origin)?;
    let mut command = tokio::process::Command::new("t3");
    command
        .args([
            "pair",
            "--label",
            "tui3",
            "--scope",
            "orchestration:read",
            "--scope",
            "orchestration:operate",
            "--base-dir",
        ])
        .arg(base)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    // Capture both streams: the CLI prints a credential and QR code that must not reach the UI/logs.
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .map_err(|_| "Local T3 pairing timed out. Press p to retry.")?
        .map_err(|_| "Could not run t3. Install the T3 CLI and make it available on PATH.")?;
    if !output.status.success() {
        return Err("T3 CLI could not pair. Check T3's connection policy and that the local server is running, then press p to retry.".into());
    }
    parse_credential(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_private_session_bound_to_origin_and_environment() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tui3/session.json");
        let origin = Url::parse("http://127.0.0.1:3773").unwrap();
        save(&path, &origin, "environment-a", "private-test-token").unwrap();
        assert_eq!(
            load(&path, &origin, "environment-a").as_deref(),
            Some("private-test-token")
        );
        assert!(load(&path, &origin, "environment-b").is_none());
        assert!(
            load(
                &path,
                &Url::parse("http://127.0.0.1:3774").unwrap(),
                "environment-a"
            )
            .is_none()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        save(&path, &origin, "environment-a", "replacement").unwrap();
        assert_eq!(
            load(&path, &origin, "environment-a").as_deref(),
            Some("replacement")
        );
    }

    #[test]
    fn parses_cli_without_returning_its_secret_output_in_errors() {
        assert_eq!(
            parse_credential(b"QR code\nToken: example-token\nExpires: date\n").unwrap(),
            "example-token"
        );
        assert!(
            !parse_credential(b"secret-output")
                .unwrap_err()
                .contains("secret-output")
        );
    }

    #[test]
    fn refuses_pairing_with_a_different_server() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("userdata");
        fs::create_dir(&state).unwrap();
        fs::write(
            state.join("server-runtime.json"),
            r#"{"origin":"http://127.0.0.1:3773"}"#,
        )
        .unwrap();
        assert!(validate_target(&state, &Url::parse("http://127.0.0.1:3774").unwrap()).is_err());
        assert!(validate_target(&state, &Url::parse("http://127.0.0.1:3773").unwrap()).is_ok());
    }
}
