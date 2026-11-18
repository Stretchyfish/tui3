//! Launch-time local T3 discovery, optional startup, and explicit installation.
use reqwest::Url;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::mpsc,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    None,
    Install,
    Start,
    Retry,
    Pair,
}
#[derive(Clone)]
pub struct Event {
    pub message: String,
    pub detail: String,
    pub offer: Offer,
    pub busy: bool,
}
#[derive(Clone, Copy)]
pub enum Command {
    Check,
    Start,
    Install,
}
pub struct State {
    pub open: bool,
    pub event: Event,
    pub request: Option<Command>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            event: event("Checking for T3…", "", Offer::None, true),
            request: None,
        }
    }
}
impl State {
    pub fn apply(&mut self, event: Event) {
        self.open |=
            event.offer != Offer::None || event.busy && event.message != "Checking for T3…";
        self.event = event;
    }
}
fn event(message: &str, detail: &str, offer: Offer, busy: bool) -> Event {
    Event {
        message: message.into(),
        detail: detail.into(),
        offer,
        busy,
    }
}
pub fn cli_path() -> Option<PathBuf> {
    let filename = if cfg!(windows) { "t3.exe" } else { "t3" };
    let mut choices: Vec<_> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(filename))
                .collect()
        })
        .unwrap_or_default();
    if let Some(directory) = std::env::var_os("T3CODE_INSTALL_BIN_DIR") {
        choices.push(PathBuf::from(directory).join(filename));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        choices.push(PathBuf::from(&home).join(".local/bin").join(filename));
        choices.push(PathBuf::from(home).join("bin").join(filename));
    }
    #[cfg(windows)]
    {
        let shims: Vec<_> = choices
            .iter()
            .map(|path| path.with_extension("cmd"))
            .collect();
        for shim in shims {
            if let Ok(text) = std::fs::read_to_string(shim) {
                if let Some(path) = text.lines().find_map(|line| {
                    line.strip_prefix('"')
                        .and_then(|line| line.strip_suffix("\" %*"))
                        .map(PathBuf::from)
                }) {
                    if path.is_absolute() && executable(&path) {
                        choices.push(path);
                    }
                }
            }
        }
    }
    choices.into_iter().find(|path| executable(path))
}
fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[derive(Clone)]
struct Target {
    origin: Url,
    state: PathBuf,
    remote: bool,
}
impl Target {
    fn load() -> Result<Self, String> {
        let state =
            crate::pairing::local_state_dir().ok_or("Cannot locate the T3 data directory.")?;
        let explicit_url = std::env::var("T3_SERVER_URL").ok();
        #[derive(Deserialize)]
        struct Runtime {
            origin: String,
        }
        let runtime = std::fs::read(state.join("server-runtime.json"))
            .ok()
            .and_then(|data| serde_json::from_slice::<Runtime>(&data).ok());
        let origin = explicit_url
            .or_else(|| runtime.map(|runtime| runtime.origin))
            .unwrap_or("http://127.0.0.1:3773".into());
        let origin = Url::parse(&origin).map_err(|_| "Invalid T3 server URL.")?;
        if !matches!(origin.scheme(), "http" | "https")
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.host_str().is_none()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err("T3 requires an HTTP(S) URL without embedded credentials.".into());
        }
        let remote = !matches!(origin.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        Ok(Self {
            origin,
            state,
            remote,
        })
    }
    fn base(&self) -> Result<&Path, String> {
        if self.state.file_name().and_then(|part| part.to_str()) != Some("userdata") {
            return Err("Local startup requires T3_STATE_DIR to end in userdata.".into());
        }
        self.state
            .parent()
            .ok_or("Invalid T3 state directory.".into())
    }
}
async fn running(client: &reqwest::Client, target: &Target) -> bool {
    let Ok(url) = target.origin.join("/.well-known/t3/environment") else {
        return false;
    };
    match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<serde_json::Value>()
            .await
            .is_ok_and(|data| {
                data["environmentId"].is_string()
                    && data["orchestrationProtocolVersion"].is_number()
            }),
        _ => false,
    }
}
fn start_command(cli: &Path, target: &Target) -> Result<tokio::process::Command, String> {
    if target.remote || target.origin.scheme() != "http" {
        return Err("Automatic startup only supports a local HTTP server.".into());
    }
    let host = if target.origin.host_str() == Some("[::1]") {
        "::1"
    } else {
        "127.0.0.1"
    };
    let mut command = tokio::process::Command::new(cli);
    command
        .args(["serve", "--mode", "desktop", "--host", host, "--port"])
        .arg(
            target
                .origin
                .port_or_known_default()
                .unwrap_or(3773)
                .to_string(),
        )
        .arg("--base-dir")
        .arg(target.base()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x08000200);
    }
    // This is a shared T3 server; closing tui3 must not shut it down.
    command.kill_on_drop(false);
    Ok(command)
}
async fn start_local(client: &reqwest::Client, target: &Target, cli: &Path) -> Result<(), String> {
    if running(client, target).await {
        return Ok(());
    }
    // Never start over an occupied port, even when its listener is not T3.
    let host = target
        .origin
        .host_str()
        .unwrap_or("127.0.0.1")
        .trim_matches(['[', ']']);
    if matches!(
        tokio::time::timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect((
                host,
                target.origin.port_or_known_default().unwrap_or(3773)
            ))
        )
        .await,
        Ok(Ok(_))
    ) {
        return Err("The configured port is occupied but did not respond as T3. Choose another server URL/port or stop that listener.".into());
    }
    let mut child = start_command(cli, target)?
        .spawn()
        .map_err(|_| "Could not launch the T3 CLI.".to_owned())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if running(client, target).await {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "T3 exited during startup ({status}). Check `t3 serve` in a terminal for details."
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("T3 has not become ready yet. It may still be starting; choose Check again before trying to start another instance.".into())
}

async fn install(updates: &mpsc::Sender<Event>) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .https_only(true)
        .build()
        .map_err(|_| "Cannot initialize installer download.".to_owned())?;
    let url = if cfg!(windows) {
        "https://t3.codes/install.ps1"
    } else {
        "https://t3.codes/install.sh"
    };
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| {
            "Could not download the official T3 installer. Check your connection.".to_owned()
        })?
        .error_for_status()
        .map_err(|_| "T3 installer download failed.".to_owned())?;
    let mut script = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Installer download was interrupted.".to_owned())?
    {
        if script.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Unexpectedly large T3 installer.".into());
        }
        script.extend_from_slice(&chunk);
    }
    let directory =
        tempfile::tempdir().map_err(|_| "Cannot create installer workspace.".to_owned())?;
    let path = directory.path().join(if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    });
    std::fs::write(&path, script).map_err(|_| "Cannot save T3 installer.".to_owned())?;
    let _ = updates.send(event("Installing T3 CLI…", "The official installer is downloading and setting up the T3 executable for this user. No system service is being installed.", Offer::None, true));
    let mut command = if cfg!(windows) {
        let mut command = tokio::process::Command::new("powershell");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
        command
    } else {
        tokio::process::Command::new("sh")
    };
    let home = crate::pairing::local_state_dir()
        .and_then(|state| state.parent().map(Path::to_owned))
        .ok_or("Cannot locate T3 home.")?;
    command
        .arg(path)
        .env("T3CODE_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(300), command.status())
        .await
        .map_err(|_| "T3 installation timed out. Check your connection and retry.".to_owned())?
        .map_err(|_| {
            "Cannot run installer. A shell (or PowerShell on Windows) is required.".to_owned()
        })?;
    if !status.success() {
        return Err("T3 installation failed. Check platform support and installer prerequisites at https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md, then retry.".into());
    }
    if cli_path().is_none() {
        return Err("Installer completed, but the t3 command was not found in PATH or ~/.local/bin. Add its install directory to PATH, then Check again.".into());
    }
    Ok(())
}

pub struct Backend {
    pub updates: mpsc::Receiver<Event>,
    pub commands: tokio::sync::mpsc::Sender<Command>,
    pub auto_start: tokio::sync::watch::Sender<bool>,
}
pub fn spawn(runtime: &tokio::runtime::Runtime, auto_start: bool) -> Backend {
    let (sender, updates) = mpsc::channel();
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    let (setting, mut settings) = tokio::sync::watch::channel(auto_start);
    runtime.spawn(async move {
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(2)).redirect(reqwest::redirect::Policy::none()).build() {
            Ok(client) => client,
            Err(_) => { let _ = sender.send(event("T3 startup check failed", "Could not initialize the connection checker.", Offer::Retry, false)); return; }
        };
        let mut next = Command::Check;
        loop {
            let _ = sender.send(event("Checking for T3…", "", Offer::None, true));
            let outcome = async {
                let target = Target::load()?;
                let was_running = running(&client, &target).await;
                if was_running && !(matches!(next, Command::Install) && cli_path().is_none()) {
                    if !target.remote && cli_path().is_none() {
                        return Ok(event("T3 is running, but its CLI is missing", "Install the official CLI to enable local pairing without copying tokens. The running server will be reused.", Offer::Install, false));
                    }
                    return Ok(event("T3 server is running", "Using the existing server.", Offer::None, false));
                }
                if target.remote { return Ok(event("Remote T3 server is unavailable", "Check its address and start it on the remote machine. Local auto-start and installation do not apply to remote URLs.", Offer::Retry, false)); }
                let mut cli = cli_path();
                if matches!(next, Command::Install) {
                    target.base()?;
                    let _ = sender.send(event("Downloading and installing T3…", "Using the official t3.codes installer for this user. This may take a few minutes.", Offer::None, true));
                    install(&sender).await?; cli = cli_path();
                }
                if was_running { return Ok(event("T3 CLI installed", "The running server was reused. Connect to save your local pairing session.", Offer::Pair, false)); }
                let Some(cli) = cli else { return Ok(event("T3 CLI/server is not installed", "Install the official T3 CLI for this user, then start its local server. No system service will be installed. Download source: https://t3.codes", Offer::Install, false)); };
                if matches!(next, Command::Start | Command::Install) || *settings.borrow() {
                    let _ = sender.send(event("T3 is not running. Starting it…", "Launching a local, loopback-only T3 server without opening a browser.", Offer::None, true));
                    start_local(&client, &target, &cli).await?;
                    Ok(event("T3 is ready", "The server will keep running when tui3 closes. Connect to save a local pairing session without copying tokens.", Offer::Pair, false))
                } else { Ok(event("T3 is installed but not running", "Automatic startup is disabled. Start it now, or close this panel and start it yourself.", Offer::Start, false)) }
            }.await;
            let outcome: Result<Event, String> = outcome;
            let _ = sender.send(outcome.unwrap_or_else(|error| event("T3 setup needs attention", &error, Offer::Retry, false)));
            tokio::select! {
                command = requests.recv() => match command { Some(command) => next = command, None => break },
                changed = settings.changed() => { if changed.is_err() { break; } next = Command::Check; }
            }
        }
    });
    Backend {
        updates,
        commands,
        auto_start: setting,
    }
}

pub fn activate(app: &mut crate::app::App) {
    if app.startup.event.busy {
        return;
    }
    app.startup.request = match app.startup.event.offer {
        Offer::Install => Some(Command::Install),
        Offer::Start => Some(Command::Start),
        Offer::Retry => Some(Command::Check),
        Offer::Pair => {
            app.pairing_requested = true;
            app.startup.open = false;
            None
        }
        Offer::None => {
            app.startup.open = false;
            None
        }
    };
}

pub fn draw(frame: &mut ratatui::Frame, app: &mut crate::app::App) {
    use ratatui::{
        layout::{Constraint, Layout},
        style::{Color, Style},
        widgets::{Clear, Paragraph, Wrap},
    };
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage(10),
        Constraint::Percentage(80),
        Constraint::Percentage(10),
    ])
    .areas(frame.area());
    let [_, area, _] = Layout::horizontal([
        Constraint::Percentage(10),
        Constraint::Percentage(80),
        Constraint::Percentage(10),
    ])
    .areas(center);
    frame.render_widget(Clear, area);
    let block = crate::ui::panel(" T3 setup ", true)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [body, buttons] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(inner);
    let e = &app.startup.event;
    let phase = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100;
    let indicator = if e.busy {
        ["⠋ ", "⠙ ", "⠹ ", "⠸ ", "⠼ ", "⠴ ", "⠦ ", "⠧ ", "⠇ ", "⠏ "][(phase % 10) as usize]
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{indicator}{}\n\n{}\n\nStart T3 when tui3 launches: {}\nChange this in Settings (s).",
            e.message,
            e.detail,
            if app.auto_start_t3 { "On" } else { "Off" }
        ))
        .wrap(Wrap { trim: false }),
        body,
    );
    let [primary, close] =
        Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)]).areas(buttons);
    let label = if e.busy {
        "Working…"
    } else {
        match e.offer {
            Offer::Install => "Install & set up",
            Offer::Start => "Start T3",
            Offer::Retry => "Check again",
            Offer::Pair => "Connect / pair",
            Offer::None => "Continue",
        }
    };
    frame.render_widget(
        Paragraph::new(label)
            .centered()
            .block(crate::ui::panel(" Enter ", !e.busy)),
        primary,
    );
    frame.render_widget(
        Paragraph::new("Close")
            .centered()
            .block(crate::ui::panel(" Esc ", false)),
        close,
    );
    if !e.busy {
        app.click_targets
            .push((primary, crate::app::ClickTarget::StartupPrimary));
    }
    app.click_targets
        .push((close, crate::app::ClickTarget::StartupClose));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(port: u16, root: &Path) -> Target {
        Target {
            origin: Url::parse(&format!("http://127.0.0.1:{port}")).unwrap(),
            state: root.join("userdata"),
            remote: false,
        }
    }
    #[test]
    fn local_command_uses_the_right_data_directory_and_remote_targets_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let mut target = target(4567, directory.path());
        let command = start_command(Path::new("fake-t3"), &target).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            &args[..7],
            [
                "serve",
                "--mode",
                "desktop",
                "--host",
                "127.0.0.1",
                "--port",
                "4567"
            ]
        );
        assert_eq!(args.last().unwrap(), &directory.path().to_string_lossy());
        assert!(!args.contains(&"--lan".into()));
        target.remote = true;
        assert!(start_command(Path::new("fake-t3"), &target).is_err());
    }
    #[tokio::test]
    async fn existing_t3_is_reused_even_without_a_cli_and_another_listener_is_not_replaced() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for is_t3 in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 2048];
                let _ = stream.read(&mut request).await.unwrap();
                let body = if is_t3 {
                    r#"{"environmentId":"test","orchestrationProtocolVersion":2}"#
                } else {
                    "not a T3 server"
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                drop(stream);
                if !is_t3 {
                    let _ = listener.accept().await.unwrap();
                }
            });
            let directory = tempfile::tempdir().unwrap();
            let result = start_local(
                &reqwest::Client::new(),
                &target(port, directory.path()),
                Path::new("/nonexistent/t3"),
            )
            .await;
            if is_t3 {
                assert!(result.is_ok());
            } else {
                assert!(result.unwrap_err().contains("occupied"));
            }
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn unavailable_or_exiting_cli_reports_failure_without_hanging() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let directory = tempfile::tempdir().unwrap();
        let result = start_local(
            &reqwest::Client::new(),
            &target(port, directory.path()),
            Path::new("/nonexistent/t3"),
        )
        .await;
        assert!(result.unwrap_err().contains("launch"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cli_early_exit_is_reported_and_does_not_wait_for_readiness_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("fake-t3");
        std::fs::write(&script, "#!/bin/sh\nexit 9\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            start_local(
                &reqwest::Client::new(),
                &target(port, directory.path()),
                &script,
            ),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().contains("exited"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn starts_a_fake_server_and_discovers_its_runtime_without_touching_real_t3() {
        use std::os::unix::fs::PermissionsExt;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("fake-t3");
        std::fs::write(&script, r#"#!/usr/bin/env python3
import sys, pathlib, json
from http.server import BaseHTTPRequestHandler, HTTPServer
args=sys.argv
port=int(args[args.index('--port')+1])
base=pathlib.Path(args[args.index('--base-dir')+1])
state=base/'userdata'; state.mkdir(parents=True,exist_ok=True)
(state/'server-runtime.json').write_text(json.dumps({'origin':f'http://127.0.0.1:{port}'}))
stop=False
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self):
  global stop
  stop=self.path=='/shutdown'
  body=b'{"environmentId":"fake","orchestrationProtocolVersion":2}'
  self.send_response(200); self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
server=HTTPServer(('127.0.0.1',port),Handler)
while not stop: server.handle_request()
server.server_close()
"#).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = target(port, directory.path());
        let client = reqwest::Client::new();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            start_local(&client, &target, &script),
        )
        .await;
        // Shut down the test child even when a readiness assertion fails.
        let _ = client
            .get(target.origin.join("/shutdown").unwrap())
            .send()
            .await;
        result.unwrap().unwrap();
        assert!(target.state.join("server-runtime.json").is_file());
    }

    #[test]
    fn setup_dialog_blocks_underlying_controls_and_small_sizes_render() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = crate::app::App::default();
        app.startup.apply(event(
            "T3 CLI/server is not installed",
            "Install the official CLI",
            Offer::Install,
            false,
        ));
        for (width, height) in [(100, 30), (30, 8), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();
            assert!(app.click_targets.iter().all(|(_, target)| matches!(
                target,
                crate::app::ClickTarget::StartupPrimary | crate::app::ClickTarget::StartupClose
            )));
        }
        assert!(app.startup.request.is_none());
    }
}
