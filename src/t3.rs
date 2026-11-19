//! HTTP contracts verified against the installed T3 Code 0.1.8 server.
use std::{env, fs, path::PathBuf, sync::mpsc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Url};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub environment_id: String,
    pub label: String,
    pub server_version: String,
    pub orchestration_protocol_version: u32,
}

#[derive(Deserialize)]
struct Session {
    authenticated: bool,
}

pub struct Connection {
    pub environment: Environment,
    pub authenticated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ThreadAction {
    Create {
        project_id: String,
        thread_id: String,
        model: Option<crate::models::ModelSelection>,
    },
    Rename {
        thread_id: String,
        title: String,
    },
    Delete {
        thread_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeModeChange {
    pub command_id: String,
    pub thread_id: String,
    pub runtime_mode: String,
}

impl RuntimeModeChange {
    pub fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "thread.runtime-mode.set",
            "commandId": self.command_id,
            "threadId": self.thread_id,
            "runtimeMode": self.runtime_mode,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalResponse {
    pub command_id: String,
    pub thread_id: String,
    pub request_id: String,
    pub decision: String,
}

impl ApprovalResponse {
    pub fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "runtime-request.respond",
            "commandId": self.command_id,
            "threadId": self.thread_id,
            "requestId": self.request_id,
            "decision": self.decision,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmRelease {
    pub version: String,
    pub database_migration: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerUpdateStage {
    Downloading,
    Installing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerCommand {
    CheckVersion {
        spec: String,
    },
    StartUpdate {
        target_version: String,
        target_database_migration: Option<u64>,
    },
}

pub enum Update {
    Editor {
        request: crate::editor::Request,
        result: Result<crate::editor::Response, String>,
    },
    Git {
        request: crate::git::Request,
        result: Result<crate::git::Snapshot, String>,
    },
    GitProgress {
        id: String,
        cwd: String,
        message: String,
    },
    RunDiff {
        thread_id: String,
        run_id: String,
        result: Result<String, String>,
    },
    Dictation(crate::dictation::Event),
    Ready(Connection),
    Failed(String),
    Pairing(String),
    Catalog(Result<crate::catalog::Catalog, String>),
    Models(Result<Vec<crate::models::ModelChoice>, String>),
    Usage(Result<Vec<crate::usage::ProviderUsage>, String>),
    ModelChanged {
        request: crate::models::ModelChange,
        result: Result<(), String>,
    },
    RuntimeModeChanged {
        request: RuntimeModeChange,
        result: Result<(), String>,
    },
    ApprovalResponded {
        request: ApprovalResponse,
        result: Result<(), String>,
    },
    ThreadAction {
        action: ThreadAction,
        result: Result<(), String>,
    },
    Sent {
        request: crate::message::SendRequest,
        result: Result<(), crate::message::SendFailure>,
    },
    Live {
        thread_id: String,
        result: Result<crate::history::LiveUpdate, String>,
    },
    History {
        thread_id: String,
        result: Result<crate::history::Transcript, String>,
    },
    ServerVersionChecked(Result<Option<NpmRelease>, String>),
    ServerUpdateProgress(ServerUpdateStage),
    ServerUpdateFinished(Result<String, String>),
}

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    origin: Url,
    access_token: Option<String>,
    bootstrap_token: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let origin = match env::var("T3_SERVER_URL") {
            Ok(value) => value,
            Err(_) => {
                let state = crate::pairing::local_state_dir()
                    .ok_or("Set T3_SERVER_URL to your T3 server origin.")?;
                #[derive(Deserialize)]
                struct Runtime {
                    origin: String,
                }
                let data = fs::read(state.join("server-runtime.json"))
                    .map_err(|_| "No local T3 runtime found. Start T3 or set T3_SERVER_URL.")?;
                serde_json::from_slice::<Runtime>(&data)
                    .map_err(|_| "Invalid T3 server-runtime.json; set T3_SERVER_URL.")?
                    .origin
            }
        };
        let origin = Url::parse(&origin).map_err(|_| "Invalid T3_SERVER_URL.")?;
        if !matches!(origin.scheme(), "http" | "https")
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
        {
            return Err(
                "T3_SERVER_URL must be an HTTP(S) origin without credentials, path, or query."
                    .into(),
            );
        }
        let access_token = match env::var_os("T3_ACCESS_TOKEN_FILE") {
            Some(path) => Some(
                fs::read_to_string(path)
                    .map_err(|_| "Could not read T3_ACCESS_TOKEN_FILE.")?
                    .trim()
                    .to_owned(),
            ),
            None => env::var("T3_ACCESS_TOKEN").ok(),
        };
        let bootstrap_token = env::var("T3_BOOTSTRAP_TOKEN").ok();
        if (access_token.is_some() || bootstrap_token.is_some())
            && origin.scheme() == "http"
            && !matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("Use HTTPS when authenticating to a remote T3 server.".into());
        }
        Ok(Self {
            origin,
            access_token,
            bootstrap_token,
        })
    }
}

fn extract_rpc_reason(exit: &serde_json::Value) -> Option<String> {
    exit["cause"]["failure"]["reason"]
        .as_str()
        .or_else(|| exit["cause"]["error"]["reason"].as_str())
        .or_else(|| exit["cause"]["failure"]["message"].as_str())
        .or_else(|| exit["cause"]["error"]["message"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub struct T3Client {
    config: Config,
    http: Client,
    session_path: Option<PathBuf>,
    saved_session_checked: bool,
}

impl T3Client {
    pub fn new(config: Config) -> Result<Self, String> {
        let http = Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "Could not initialize HTTP client.")?;
        Ok(Self {
            config,
            http,
            session_path: crate::pairing::session_path().ok(),
            saved_session_checked: false,
        })
    }

    async fn response(&self, path: &str, authenticate: bool) -> Result<reqwest::Response, String> {
        let mut request = self
            .http
            .get(self.config.origin.join(path).expect("constant API path"));
        if path.starts_with("/api/orchestration/") {
            request = request.header("x-t3-orchestration-protocol", "2");
        }
        if authenticate && let Some(token) = &self.config.access_token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                "T3 server timed out."
            } else {
                "Cannot reach T3 server."
            }
        })?;
        Ok(response)
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        authenticate: bool,
    ) -> Result<T, String> {
        let response = self.response(path, authenticate).await?;
        if !response.status().is_success() {
            return Err(format!(
                "T3 request failed (HTTP {}). Check server address and credentials.",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "Unexpected T3 response format. Check server compatibility.".into())
    }

    async fn catalog(&self) -> Result<crate::catalog::Catalog, String> {
        let mut catalog = self
            .json::<crate::catalog::Catalog>("/api/orchestration/shell", true)
            .await?;
        catalog.normalize();
        Ok(catalog)
    }

    async fn transcript(&self, thread_id: &str) -> Result<crate::history::Transcript, String> {
        // Encode the ID as one path segment, including slashes and query characters.
        let mut path =
            Url::parse("http://localhost/api/orchestration/threads/").expect("constant URL");
        path.path_segments_mut()
            .expect("HTTP URL")
            .pop_if_empty()
            .push(thread_id)
            .push("transcript");
        let transcript = self
            .json::<crate::history::Transcript>(path.path(), true)
            .await?;
        if transcript.thread_id != thread_id {
            return Err("T3 returned history for a different thread.".into());
        }
        Ok(transcript)
    }

    async fn websocket(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        String,
    > {
        let mut url = self.config.origin.join("/ws").expect("constant API path");
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| "Invalid T3 WebSocket URL.")?;
        url.query_pairs_mut()
            .append_pair("orchestrationProtocol", "2");
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid T3 WebSocket request.")?;
        if let Some(token) = &self.config.access_token {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| "Invalid T3 credentials.")?,
            );
        }
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(request),
        )
        .await
        .map_err(|_| "T3 WebSocket connection timed out.")?
        .map_err(|_| "Could not open authenticated T3 WebSocket. Check connection and sign-in.")?;
        Ok(socket)
    }

    async fn stream_history(
        &self,
        thread_id: &str,
        updates: &mpsc::Sender<Update>,
    ) -> Result<(), String> {
        self.stream_rpc(
            "orchestration.subscribeThread",
            serde_json::json!({"threadId":thread_id,"requestCompletionMarker":true}),
            updates,
            |value| {
                let update: crate::history::LiveUpdate =
                    serde_json::from_value(value).map_err(|_| "Invalid T3 thread update.")?;
                if let crate::history::LiveUpdate::Snapshot { projection, .. } = &update
                    && projection.thread.id != thread_id
                {
                    return Err("T3 streamed a different thread.".into());
                }
                if let crate::history::LiveUpdate::Event { event, .. } = &update
                    && event.kind == "turn-item.updated"
                    && (event.payload.get("id").and_then(|id| id.as_str()).is_none()
                        || serde_json::from_value::<crate::history::Item>(event.payload.clone())
                            .is_err())
                {
                    return Err("Invalid T3 live message item.".into());
                }
                Ok(Some(Update::Live {
                    thread_id: thread_id.into(),
                    result: Ok(update),
                }))
            },
        )
        .await
    }

    async fn stream_usage(&self, updates: &mpsc::Sender<Update>) -> Result<(), String> {
        self.stream_rpc(
            "subscribeServerConfig",
            serde_json::json!({"workspaceSnapshotsOnDemand":true}),
            updates,
            |value| {
                crate::usage::from_config_event(value)
                    .map(|providers| providers.map(|providers| Update::Usage(Ok(providers))))
            },
        )
        .await
    }

    async fn stream_rpc(
        &self,
        method: &str,
        payload: serde_json::Value,
        updates: &mpsc::Sender<Update>,
        mut decode: impl FnMut(serde_json::Value) -> Result<Option<Update>, String>,
    ) -> Result<(), String> {
        let mut socket = self.websocket().await?;
        let request = serde_json::json!({"_tag":"Request","id":"1","tag":method,"payload":payload,"headers":[]});
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|_| "Could not subscribe to T3 thread.")?;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;
        let mut received = tokio::time::Instant::now();
        loop {
            let frame = tokio::select! {
                frame = socket.next() => frame.ok_or("T3 live connection closed.")?.map_err(|_| "T3 live connection lost.")?,
                _ = heartbeat.tick() => {
                    if received.elapsed() > Duration::from_secs(45) { return Err("T3 live connection stopped responding.".into()); }
                    socket.send(Message::Text(r#"{"_tag":"Ping"}"#.into())).await.map_err(|_| "T3 live connection lost.")?;
                    continue;
                }
            };
            received = tokio::time::Instant::now();
            let data = match frame {
                Message::Text(text) => text.as_bytes().to_vec(),
                Message::Binary(data) => data.to_vec(),
                Message::Ping(data) => {
                    socket
                        .send(Message::Pong(data))
                        .await
                        .map_err(|_| "T3 live connection lost.")?;
                    continue;
                }
                Message::Close(_) => return Err("T3 live connection closed.".into()),
                _ => continue,
            };
            let value: serde_json::Value =
                serde_json::from_slice(&data).map_err(|_| "Invalid T3 live response.")?;
            let messages = value.as_array().cloned().unwrap_or_else(|| vec![value]);
            for message in messages {
                match message["_tag"].as_str() {
                    Some("Ping") => socket
                        .send(Message::Text(r#"{"_tag":"Pong"}"#.into()))
                        .await
                        .map_err(|_| "T3 live connection lost.")?,
                    Some("Chunk") if message["requestId"] == "1" => {
                        let values = message["values"]
                            .as_array()
                            .ok_or("Invalid T3 stream chunk.")?;
                        for value in values {
                            if let Some(update) = decode(value.clone())? {
                                updates.send(update).map_err(|_| "TUI closed.")?;
                            }
                        }
                        // Effect RPC requires an acknowledgement for each streaming chunk.
                        socket
                            .send(Message::Text(r#"{"_tag":"Ack","requestId":"1"}"#.into()))
                            .await
                            .map_err(|_| "T3 live acknowledgement failed.")?;
                    }
                    Some("Exit") if message["requestId"] == "1" => {
                        return Err("T3 subscription ended. Reconnecting.".into());
                    }
                    Some("Defect") => {
                        return Err("T3 could not process the live subscription.".into());
                    }
                    _ => (),
                }
            }
        }
    }

    async fn rpc(
        &self,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, crate::message::SendFailure> {
        let operation = async {
            let mut socket = self.websocket().await?;
            let request = serde_json::json!({"_tag":"Request","id":"1","tag":method,"payload":payload,"headers":[]});
            socket.send(Message::Text(request.to_string().into())).await
                .map_err(|_| crate::message::SendFailure::uncertain("Connection lost while sending. Delivery is uncertain; check history before retrying."))?;
            while let Some(frame) = socket.next().await {
                let frame = frame.map_err(|_| crate::message::SendFailure::uncertain("Connection lost before acknowledgement. Delivery is uncertain; check history before retrying."))?;
                let data = match frame {
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Binary(data) => data.to_vec(),
                    Message::Ping(data) => {
                        socket
                            .send(Message::Pong(data))
                            .await
                            .map_err(|_| crate::message::SendFailure::uncertain("T3 connection lost before acknowledgement. Check history before retrying."))?;
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_slice(&data).map_err(|_| {
                    crate::message::SendFailure::uncertain(
                        "Invalid T3 acknowledgement. Check history before retrying.",
                    )
                })?;
                let messages = value.as_array().cloned().unwrap_or_else(|| vec![value]);
                for message in messages {
                    if message["_tag"] == "Ping" {
                        socket
                            .send(Message::Text(r#"{"_tag":"Pong"}"#.into()))
                            .await
                            .map_err(|_| crate::message::SendFailure::uncertain("T3 connection lost before acknowledgement. Check history before retrying."))?;
                    } else if message["_tag"] == "Exit" && message["requestId"] == "1" {
                        let result = if message["exit"]["_tag"] == "Success" {
                            Ok(message["exit"]["value"].clone())
                        } else {
                            // Server failure objects can contain prompts and credentials; don't log them.
                            Err(
                                "T3 rejected the operation. Check thread status and permissions."
                                    .into(),
                            )
                        };
                        let _ = socket.close(None).await;
                        return result;
                    } else if message["_tag"] == "Defect" {
                        return Err(
                            "T3 could not process this operation. Your draft is kept.".into()
                        );
                    }
                }
            }
            Err(crate::message::SendFailure::uncertain(
                "T3 closed before acknowledgement. Delivery is uncertain; check history before retrying.",
            ))
        };
        tokio::time::timeout(Duration::from_secs(30), operation).await
            .map_err(|_| crate::message::SendFailure::uncertain("T3 acknowledgement timed out. Delivery is uncertain; check history before retrying."))?
    }

    /// Check that T3's local speech service can transcribe right now.
    async fn voice_ready(&self) -> Result<(), String> {
        #[derive(Deserialize)]
        struct Status {
            supported: bool,
            state: Option<String>,
        }
        let status = self.json::<Status>("/api/voice/status", true).await?;
        match (status.supported, status.state.as_deref()) {
            (false, _) => Err("T3 speech transcription isn't supported on this server's platform.".into()),
            (true, Some("missing-model")) => Err(
                "T3 has no speech model yet. Download one in T3 Code → Settings → Voice, then press Shift+D again.".into(),
            ),
            _ => Ok(()),
        }
    }

    /// Transcribe 16 kHz mono f32le PCM through `POST /api/voice/transcribe`.
    async fn transcribe(&self, pcm: &[u8]) -> Result<String, String> {
        let mut request = self
            .http
            .post(
                self.config
                    .origin
                    .join("/api/voice/transcribe")
                    .expect("constant API path"),
            )
            .timeout(Duration::from_secs(130))
            .header("content-type", "application/octet-stream")
            .body(crate::dictation::transcribe_body(pcm));
        if let Some(token) = &self.config.access_token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                "T3 transcription timed out."
            } else {
                "Cannot reach T3 server for transcription."
            }
        })?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            let code = body["code"]
                .as_str()
                .or(body["reason"].as_str())
                .unwrap_or("");
            return Err(match code {
                "speech_busy" => "T3's speech service is busy. Try again in a moment.".into(),
                "speech_unavailable" => {
                    "T3 speech transcription is unavailable on this server.".into()
                }
                "invalid_audio" => "T3 rejected the recorded audio.".into(),
                _ => format!("T3 transcription failed (HTTP {}).", status.as_u16()),
            });
        }
        body["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Unexpected T3 transcription response.".into())
    }

    async fn editor_request(
        &self,
        request: &crate::editor::Request,
    ) -> Result<crate::editor::Response, String> {
        use crate::editor::{Entry, Operation, Response};
        match &request.operation {
            Operation::ListEntries(directory) => {
                let value = self
                    .rpc("projects.listEntries", request.payload())
                    .await
                    .map_err(|error| error.message)?;
                let entries = value
                    .get("entries")
                    .cloned()
                    .ok_or("Invalid T3 file list.")?;
                let entries = serde_json::from_value::<Vec<Entry>>(entries)
                    .map_err(|_| "Invalid T3 file list.")?;
                let truncated = value
                    .get("truncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(Response::Entries {
                    directory: directory.clone(),
                    entries,
                    truncated,
                })
            }
            Operation::ReadFile(relative_path) => {
                let value = self
                    .rpc("projects.readFile", request.payload())
                    .await
                    .map_err(|error| error.message)?;
                let contents = value
                    .get("contents")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let truncated = value
                    .get("truncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let exists = value.get("exists").and_then(|v| v.as_bool()).unwrap_or(true);
                Ok(Response::File {
                    relative_path: relative_path.clone(),
                    contents,
                    truncated,
                    exists,
                })
            }
            Operation::WriteFile { path, .. } => {
                self.rpc("projects.writeFile", request.payload())
                    .await
                    .map_err(|error| error.message)?;
                Ok(Response::Saved {
                    relative_path: path.clone(),
                })
            }
        }
    }

    async fn git_request(
        &self,
        request: &crate::git::Request,
        updates: &mpsc::Sender<Update>,
    ) -> Result<crate::git::Snapshot, String> {
        use crate::git::Operation;
        match &request.operation {
            Operation::Refresh | Operation::Generate(_) => (),
            Operation::Switch(_) => {
                self.rpc("vcs.switchRef", request.payload())
                    .await
                    .map_err(|error| error.message)?;
            }
            Operation::Commit(_) | Operation::CommitPush(_) | Operation::Push => {
                let mut completed = None;
                let stream = self.stream_rpc(
                    "git.runStackedAction",
                    request.payload(),
                    updates,
                    |value| {
                        if value["actionId"].as_str() != Some(&request.id) {
                            return Ok(None);
                        }
                        match value["kind"].as_str() {
                            Some("action_finished") => completed = Some(Ok(())),
                            Some("action_failed") => {
                                completed = Some(Err(value["message"]
                                    .as_str()
                                    .unwrap_or("Git action failed.")
                                    .to_owned()))
                            }
                            Some("phase_started") => {
                                return Ok(Some(Update::GitProgress {
                                    id: request.id.clone(),
                                    cwd: request.cwd.clone(),
                                    message: value["label"].as_str().unwrap_or("Working…").into(),
                                }));
                            }
                            _ => (),
                        }
                        Ok(None)
                    },
                );
                let ended = tokio::time::timeout(Duration::from_secs(180), stream).await;
                match completed {
                    Some(result) => result?,
                    None => {
                        return Err(format!(
                            "{} Check the repository before retrying; the action may have completed.",
                            match ended {
                                Ok(Err(error)) => error,
                                _ => "Git action ended without a completion result.".into(),
                            }
                        ));
                    }
                }
            }
        }
        let status = self
            .rpc("vcs.refreshStatus", serde_json::json!({"cwd":request.cwd}))
            .await
            .map_err(|error| error.message)?;
        let status = serde_json::from_value::<crate::git::Status>(status)
            .map_err(|_| "Invalid T3 repository status.".to_owned())?;
        let mut branches = Vec::new();
        if status.is_repo {
            let mut cursor = 0;
            loop {
                let value = self.rpc("vcs.listRefs", serde_json::json!({"cwd":request.cwd,"refKind":"local","limit":200,"cursor":cursor})).await.map_err(|error| error.message)?;
                if let Some(refs) = value["refs"].as_array() {
                    branches.extend(
                        refs.iter()
                            .filter_map(|entry| entry["name"].as_str().map(str::to_owned)),
                    );
                }
                match value["nextCursor"].as_u64() {
                    Some(next) if next > cursor => cursor = next,
                    _ => break,
                }
            }
        }
        let generated_message = if matches!(request.operation, Operation::Generate(_)) {
            if status.working_tree.files.is_empty() {
                return Err("No changes to describe.".into());
            }
            let value = self.rpc("review.getDiffPreview", serde_json::json!({
                "cwd":request.cwd, "sourceKind":"working-tree", "trackedOnly":false, "metadataOnly":false, "ignoreWhitespace":false
            })).await.map_err(|error| format!("{} If access was denied, press p in Git to refresh pairing permissions, then Generate again.", error.message))?;
            let patch = value["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|source| source["kind"] == "working-tree")
                .filter_map(|source| source["diff"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if patch.trim().is_empty() {
                return Err("T3 returned no text diff to summarize. Your draft is kept.".into());
            }
            let prompt = crate::commit_message::prompt(
                status.ref_name.as_deref(),
                &status.working_tree.files,
                &patch,
            );
            Some(crate::commit_message::generate(&prompt).await?)
        } else {
            None
        };
        Ok(crate::git::Snapshot {
            status,
            branches,
            generated_message,
        })
    }

    async fn available_models(&self) -> Result<Vec<crate::models::ModelChoice>, String> {
        let config = self
            .rpc("server.getConfig", serde_json::json!({}))
            .await
            .map_err(|error| error.message)?;
        crate::models::choices(config)
    }

    async fn change_model(&self, request: &crate::models::ModelChange) -> Result<(), String> {
        let value = self
            .rpc("orchestration.dispatchCommand", request.payload())
            .await
            .map_err(|error| {
                if error.retry_same_id {
                    "Could not confirm model change. Reopen the selector to check or retry.".into()
                } else {
                    error.message
                }
            })?;
        if value["sequence"].as_u64().is_none() {
            return Err(
                "Unexpected model-change acknowledgement. Reopen the selector to check.".into(),
            );
        }
        Ok(())
    }

    async fn change_runtime_mode(&self, request: &RuntimeModeChange) -> Result<(), String> {
        let value = self
            .rpc("orchestration.dispatchCommand", request.payload())
            .await
            .map_err(|error| error.message)?;
        if value["sequence"].as_u64().is_none() {
            return Err("Unexpected access-level acknowledgement.".into());
        }
        Ok(())
    }

    async fn respond_approval(&self, request: &ApprovalResponse) -> Result<(), String> {
        let value = self
            .rpc("orchestration.dispatchCommand", request.payload())
            .await
            .map_err(|error| error.message)?;
        if value["sequence"].as_u64().is_none() {
            return Err("Unexpected approval-response acknowledgement.".into());
        }
        Ok(())
    }

    async fn check_npm_release(&self, spec: &str) -> Result<Option<NpmRelease>, String> {
        let mut socket = self.websocket().await?;
        let request = serde_json::json!({
            "_tag": "Request",
            "id": "1",
            "tag": "server.getNpmRelease",
            "payload": { "spec": spec },
            "headers": [],
        });
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|_| "Could not check T3 server version.".to_owned())?;
        let operation = async {
            while let Some(frame) = socket.next().await {
                let frame = frame
                    .map_err(|_| "Connection lost while checking server version.".to_owned())?;
                let data = match frame {
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Binary(data) => data.to_vec(),
                    Message::Ping(data) => {
                        socket.send(Message::Pong(data)).await.map_err(|_| {
                            "T3 connection lost while checking server version.".to_owned()
                        })?;
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_slice(&data)
                    .map_err(|_| "Invalid T3 version response.".to_owned())?;
                let messages = value.as_array().cloned().unwrap_or_else(|| vec![value]);
                for message in messages {
                    if message["_tag"] == "Ping" {
                        socket
                            .send(Message::Text(r#"{"_tag":"Pong"}"#.into()))
                            .await
                            .map_err(|_| {
                                "T3 connection lost while checking server version.".to_owned()
                            })?;
                    } else if message["_tag"] == "Exit" && message["requestId"] == "1" {
                        let result = if message["exit"]["_tag"] == "Success" {
                            let val = &message["exit"]["value"];
                            if val.is_null() {
                                Ok(None)
                            } else {
                                let version =
                                    val["version"].as_str().map(str::to_owned).ok_or_else(
                                        || "T3 returned invalid release metadata.".to_owned(),
                                    )?;
                                let database_migration = val["databaseMigration"].as_u64();
                                Ok(Some(NpmRelease {
                                    version,
                                    database_migration,
                                }))
                            }
                        } else {
                            Err(extract_rpc_reason(&message["exit"])
                                .unwrap_or_else(|| "Could not check T3 server version.".into()))
                        };
                        let _ = socket.close(None).await;
                        return result;
                    } else if message["_tag"] == "Defect" {
                        return Err("T3 could not check server version.".into());
                    }
                }
            }
            Err("T3 closed before returning version information.".into())
        };
        tokio::time::timeout(Duration::from_secs(20), operation)
            .await
            .map_err(|_| "T3 version check timed out.".to_owned())?
    }

    async fn update_server_with_progress(
        &self,
        target_version: &str,
        target_database_migration: Option<u64>,
        updates: &mpsc::Sender<Update>,
    ) -> Result<String, String> {
        let mut payload = serde_json::json!({
            "targetVersion": target_version,
        });
        if let Some(migration) = target_database_migration {
            payload["targetDatabaseMigration"] = serde_json::json!(migration);
        }
        let mut socket = self.websocket().await?;
        let request = serde_json::json!({
            "_tag": "Request",
            "id": "1",
            "tag": "server.updateServerWithProgress",
            "payload": payload,
            "headers": [],
        });
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|_| "Could not start T3 server update.".to_owned())?;

        let stream = async {
            let mut completed_version: Option<String> = None;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            heartbeat.tick().await;
            let mut received = tokio::time::Instant::now();
            loop {
                let frame = tokio::select! {
                    frame = socket.next() => frame
                        .ok_or_else(|| "T3 connection closed during update.".to_owned())?
                        .map_err(|_| "T3 connection lost during update.".to_owned())?,
                    _ = heartbeat.tick() => {
                        if received.elapsed() > Duration::from_secs(60) {
                            return Err("T3 server update stopped responding.".into());
                        }
                        socket
                            .send(Message::Text(r#"{"_tag":"Ping"}"#.into()))
                            .await
                            .map_err(|_| "T3 connection lost during update.".to_owned())?;
                        continue;
                    }
                };
                received = tokio::time::Instant::now();
                let data = match frame {
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Binary(data) => data.to_vec(),
                    Message::Ping(data) => {
                        socket
                            .send(Message::Pong(data))
                            .await
                            .map_err(|_| "T3 connection lost during update.".to_owned())?;
                        continue;
                    }
                    Message::Close(_) => {
                        return if let Some(version) = completed_version {
                            Ok(version)
                        } else {
                            Err("T3 connection closed before update completed.".into())
                        };
                    }
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_slice(&data)
                    .map_err(|_| "Invalid T3 update response.".to_owned())?;
                let messages = value.as_array().cloned().unwrap_or_else(|| vec![value]);
                for message in messages {
                    match message["_tag"].as_str() {
                        Some("Ping") => {
                            socket
                                .send(Message::Text(r#"{"_tag":"Pong"}"#.into()))
                                .await
                                .map_err(|_| "T3 connection lost during update.".to_owned())?;
                        }
                        Some("Chunk") if message["requestId"] == "1" => {
                            let values = message["values"]
                                .as_array()
                                .ok_or_else(|| "Invalid T3 update stream chunk.".to_owned())?;
                            for value in values {
                                match value["type"].as_str() {
                                    Some("progress") => {
                                        let stage = match value["stage"].as_str() {
                                            Some("downloading") => {
                                                Some(ServerUpdateStage::Downloading)
                                            }
                                            Some("installing") => {
                                                Some(ServerUpdateStage::Installing)
                                            }
                                            _ => None,
                                        };
                                        if let Some(stage) = stage {
                                            updates
                                                .send(Update::ServerUpdateProgress(stage))
                                                .map_err(|_| "TUI closed.".to_owned())?;
                                        }
                                    }
                                    Some("complete") => {
                                        let ver = value["result"]["targetVersion"]
                                            .as_str()
                                            .unwrap_or(target_version)
                                            .to_owned();
                                        completed_version = Some(ver);
                                    }
                                    _ => (),
                                }
                            }
                            socket
                                .send(Message::Text(r#"{"_tag":"Ack","requestId":"1"}"#.into()))
                                .await
                                .map_err(|_| "T3 update acknowledgement failed.".to_owned())?;
                        }
                        Some("Exit") if message["requestId"] == "1" => {
                            let _ = socket.close(None).await;
                            if message["exit"]["_tag"] == "Success" {
                                return Ok(
                                    completed_version.unwrap_or_else(|| target_version.to_owned())
                                );
                            }
                            return Err(extract_rpc_reason(&message["exit"])
                                .unwrap_or_else(|| "T3 server update failed.".into()));
                        }
                        Some("Defect") => {
                            return Err("T3 could not process the server update.".into());
                        }
                        _ => (),
                    }
                }
            }
        };

        tokio::time::timeout(Duration::from_secs(180), stream)
            .await
            .map_err(|_| "T3 server update timed out.".to_owned())?
    }

    async fn send_message(
        &self,
        request: &crate::message::SendRequest,
    ) -> Result<(), crate::message::SendFailure> {
        let mut payload = request.payload();
        if !request.attachments.is_empty() {
            let uploaded = self
                .rpc(
                    "assets.persistChatAttachments",
                    serde_json::json!({
                        "threadId": request.thread_id, "messageId": request.message_id,
                        "attachments": request.attachments
                    }),
                )
                .await?;
            payload["attachments"] = uploaded
                .get("attachments")
                .filter(|v| v.is_array())
                .cloned()
                .ok_or("T3 returned invalid attachment references. Your draft is kept.")?;
        }
        let result = self.rpc("orchestration.dispatchCommand", payload).await?;
        if result["sequence"].as_u64().is_none() {
            return Err(crate::message::SendFailure::uncertain(
                "Unexpected T3 acknowledgement. Check history before retrying.",
            ));
        }
        Ok(())
    }

    pub async fn run_thread_action(&self, action: &ThreadAction) -> Result<(), String> {
        let command_id = uuid::Uuid::new_v4().to_string();
        let payload = match action {
            ThreadAction::Create {
                project_id,
                thread_id,
                model,
            } => {
                let model = match model {
                    Some(model) => model.clone(),
                    None => self.available_models().await?.into_iter().next()
                        .ok_or("No available model. Configure a provider in T3 before creating a thread.")?.selection,
                };
                let payload = serde_json::json!({
                    "type": "thread.create", "commandId": command_id,
                    "threadId": thread_id, "projectId": project_id, "title": "New thread",
                    "createdBy": "user", "creationSource": "web", "branch": null, "worktreePath": null,
                    "runtimeMode": "approval-required", "interactionMode": "default", "modelSelection": model
                });
                payload
            }
            ThreadAction::Rename { thread_id, title } => {
                let title = title.trim();
                if title.is_empty() {
                    return Err("Thread title cannot be empty.".into());
                }
                serde_json::json!({
                    "type": "thread.metadata.update",
                    "commandId": command_id,
                    "threadId": thread_id,
                    "title": title,
                })
            }
            ThreadAction::Delete { thread_id } => serde_json::json!({
                "type": "thread.delete",
                "commandId": command_id,
                "threadId": thread_id,
            }),
        };
        let result = self
            .rpc("orchestration.dispatchCommand", payload)
            .await
            .map_err(|error| error.message)?;
        if result["sequence"].as_u64().is_none() {
            return Err("Unexpected T3 acknowledgement.".into());
        }
        Ok(())
    }

    async fn exchange_bootstrap(&mut self) -> Result<(), String> {
        if self.config.access_token.is_none()
            && let Some(bootstrap) = self.config.bootstrap_token.take()
        {
            #[derive(Deserialize)]
            struct Token {
                access_token: String,
                token_type: String,
            }
            let response = self
                .http
                .post(
                    self.config
                        .origin
                        .join("/oauth/token")
                        .expect("constant API path"),
                )
                .form(&[
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:token-exchange",
                    ),
                    ("subject_token", bootstrap.as_str()),
                    (
                        "subject_token_type",
                        "urn:t3:params:oauth:token-type:environment-bootstrap",
                    ),
                    (
                        "requested_token_type",
                        "urn:ietf:params:oauth:token-type:access_token",
                    ),
                    ("client_label", "tui3"),
                ])
                .send()
                .await
                .map_err(|_| "T3 token exchange failed.")?;
            if !response.status().is_success() {
                return Err("T3 rejected the bootstrap credential.".into());
            }
            let token = response
                .json::<Token>()
                .await
                .map_err(|_| "Invalid T3 token response.")?;
            if token.token_type != "Bearer" {
                return Err(
                    "This client requires a Bearer credential; DPoP is not supported yet.".into(),
                );
            }
            self.config.access_token = Some(token.access_token);
        }
        Ok(())
    }

    async fn pair(&mut self) -> Result<Connection, String> {
        // Check protocol and server identity before creating a pairing credential.
        let previous = self
            .json::<Environment>("/.well-known/t3/environment", false)
            .await?;
        if previous.orchestration_protocol_version != 2 {
            return Err("Unsupported T3 orchestration protocol; expected version 2.".into());
        }
        let path = self
            .session_path
            .clone()
            .ok_or("Cannot locate private session storage.")?;
        let credential = crate::pairing::local_credential(&self.config.origin).await?;
        self.config.access_token = None;
        self.config.bootstrap_token = Some(credential);
        let connection = self.check().await?;
        if connection.environment.environment_id != previous.environment_id {
            self.config.access_token = None;
            return Err("T3 server identity changed during pairing. Please retry.".into());
        }
        if !connection.authenticated {
            return Err("T3 did not accept the new session. Press p to retry.".into());
        }
        let saved = crate::pairing::save(
            &path,
            &self.config.origin,
            &connection.environment.environment_id,
            self.config
                .access_token
                .as_deref()
                .ok_or("Pairing produced no session.")?,
        );
        if let Err(error) = saved {
            self.config.access_token = None;
            return Err(error);
        }
        Ok(connection)
    }

    pub async fn check(&mut self) -> Result<Connection, String> {
        let environment = self
            .json::<Environment>("/.well-known/t3/environment", false)
            .await?;
        if environment.orchestration_protocol_version != 2 {
            return Err("Unsupported T3 orchestration protocol; expected version 2.".into());
        }
        if !self.saved_session_checked {
            self.saved_session_checked = true;
            if self.config.access_token.is_none()
                && self.config.bootstrap_token.is_none()
                && let Some(path) = &self.session_path
            {
                self.config.access_token =
                    crate::pairing::load(path, &self.config.origin, &environment.environment_id);
            }
        }
        self.exchange_bootstrap().await?;
        let response = self.response("/api/auth/session", true).await?;
        let session = if matches!(response.status().as_u16(), 401 | 403) {
            Session {
                authenticated: false,
            }
        } else if response.status().is_success() {
            response
                .json::<Session>()
                .await
                .map_err(|_| "Invalid T3 session response.")?
        } else {
            return Err(format!(
                "T3 session check failed (HTTP {}).",
                response.status().as_u16()
            ));
        };
        Ok(Connection {
            environment,
            authenticated: session.authenticated,
        })
    }
}

pub struct Backend {
    pub editor: tokio::sync::mpsc::Sender<crate::editor::Request>,
    pub git: tokio::sync::mpsc::Sender<crate::git::Request>,
    pub diffs: tokio::sync::mpsc::Sender<(String, String, u64)>,
    pub updates: mpsc::Receiver<Update>,
    pub pairing: tokio::sync::mpsc::Sender<()>,
    pub selected_thread: tokio::sync::watch::Sender<Option<String>>,
    pub messages: tokio::sync::mpsc::Sender<crate::message::SendRequest>,
    pub models: tokio::sync::mpsc::Sender<crate::models::ModelCommand>,
    pub runtime_modes: tokio::sync::mpsc::Sender<RuntimeModeChange>,
    pub approvals: tokio::sync::mpsc::Sender<ApprovalResponse>,
    pub dictation: tokio::sync::mpsc::Sender<crate::dictation::Command>,
    pub thread_actions: tokio::sync::mpsc::Sender<ThreadAction>,
    pub server_updates: tokio::sync::mpsc::Sender<ServerCommand>,
}

pub fn start(runtime: &tokio::runtime::Runtime) -> Backend {
    let (sender, receiver) = mpsc::channel();
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    let (selected_thread, mut selection) = tokio::sync::watch::channel::<Option<String>>(None);
    let history_sender = sender.clone();
    let (credentials, mut authorization) = tokio::sync::watch::channel::<Option<Config>>(None);
    let mut usage_authorization = authorization.clone();
    let editor_auth = authorization.clone();
    let editor_updates = sender.clone();
    let (editor, mut editor_requests) = tokio::sync::mpsc::channel::<crate::editor::Request>(4);
    runtime.spawn(async move {
        while let Some(request) = editor_requests.recv().await {
            let config = editor_auth.borrow().clone();
            let result = match config
                .ok_or("Sign in to T3 first.".to_owned())
                .and_then(T3Client::new)
            {
                Ok(client) => client.editor_request(&request).await,
                Err(error) => Err(error),
            };
            if editor_updates
                .send(Update::Editor { request, result })
                .is_err()
            {
                break;
            }
        }
    });
    let git_auth = authorization.clone();
    let git_updates = sender.clone();
    let (git, mut git_requests) = tokio::sync::mpsc::channel::<crate::git::Request>(4);
    runtime.spawn(async move {
        while let Some(request) = git_requests.recv().await {
            let config = git_auth.borrow().clone();
            let result = match config
                .ok_or("Sign in to T3 first.".to_owned())
                .and_then(T3Client::new)
            {
                Ok(client) => client.git_request(&request, &git_updates).await,
                Err(error) => Err(error),
            };
            if git_updates.send(Update::Git { request, result }).is_err() {
                break;
            }
        }
    });
    let diff_auth = authorization.clone();
    let diff_updates = sender.clone();
    let (diffs, mut diff_requests) = tokio::sync::mpsc::channel::<(String, String, u64)>(32);
    runtime.spawn(async move {
        while let Some((thread_id, run_id, ordinal)) = diff_requests.recv().await {
            let config = diff_auth.borrow().clone();
            let result = match config.ok_or("Sign in to T3 first.".to_owned()).and_then(T3Client::new) {
                Ok(client) => {
                    let payload = serde_json::json!({
                        "threadId":thread_id, "fromTurnCount":ordinal.saturating_sub(1), "toTurnCount":ordinal, "ignoreWhitespace": false
                    });
                    let mut response = client.rpc("orchestration.getTurnDiff", payload.clone()).await;
                    for delay in [1, 2] {
                        if response.is_ok() { break; }
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        response = client.rpc("orchestration.getTurnDiff", payload.clone()).await;
                    }
                    response.map_err(|error| error.message).and_then(|value| {
                    value.get("diff").and_then(|diff| diff.as_str()).map(str::to_owned).ok_or("T3 returned no diff.".into())
                    })
                },
                Err(error) => Err(error),
            };
            if diff_updates.send(Update::RunDiff { thread_id, run_id, result }).is_err() { break; }
        }
    });
    let usage_updates = sender.clone();
    runtime.spawn(async move {
        let mut delay = 1_u64;
        loop {
            let config = usage_authorization.borrow_and_update().clone();
            if let Some(config) = config {
                let started = tokio::time::Instant::now();
                let stream = async {
                    let result = match T3Client::new(config) { Ok(client) => client.stream_usage(&usage_updates).await, Err(error) => Err(error) };
                    if let Err(error) = result { let _ = usage_updates.send(Update::Usage(Err(error))); }
                };
                tokio::select! {
                    _ = stream => (),
                    changed = usage_authorization.changed() => { if changed.is_err() { break; } delay = 1; continue; }
                }
                if started.elapsed() > Duration::from_secs(30) { delay = 1; }
            } else {
                let _ = usage_updates.send(Update::Usage(Err("Waiting for T3 connection.".into())));
            }
            tokio::select! {
                changed = usage_authorization.changed() => { if changed.is_err() { break; } delay = 1; }
                _ = tokio::time::sleep(Duration::from_secs(delay)) => { delay = (delay * 2).min(15); }
            }
        }
    });
    let model_authorization = authorization.clone();
    let model_updates = sender.clone();
    let runtime_mode_authorization = authorization.clone();
    let runtime_mode_updates = sender.clone();
    let approval_authorization = authorization.clone();
    let approval_updates = sender.clone();
    let server_update_authorization = authorization.clone();
    let server_update_updates = sender.clone();
    let (models, mut model_requests) = tokio::sync::mpsc::channel::<crate::models::ModelCommand>(2);
    runtime.spawn(async move {
        while let Some(command) = model_requests.recv().await {
            let config = model_authorization.borrow().clone();
            let client = config
                .ok_or_else(|| "Sign in to T3 first.".to_owned())
                .and_then(T3Client::new);
            let update = match command {
                crate::models::ModelCommand::Load => Update::Models(match client {
                    Ok(client) => client.available_models().await,
                    Err(error) => Err(error),
                }),
                crate::models::ModelCommand::Set(request) => {
                    let result = match client {
                        Ok(client) => client.change_model(&request).await,
                        Err(error) => Err(error),
                    };
                    Update::ModelChanged { request, result }
                }
            };
            if model_updates.send(update).is_err() {
                break;
            }
        }
    });
    let dictation_authorization = authorization.clone();
    let dictation_updates = sender.clone();
    let (dictation, mut dictation_commands) =
        tokio::sync::mpsc::channel::<crate::dictation::Command>(4);
    runtime.spawn(async move {
        use crate::dictation::{Command, Event, Recorder};
        let send = |event| dictation_updates.send(Update::Dictation(event)).is_ok();
        let mut recording: Option<(Recorder, T3Client)> = None;
        while let Some(command) = dictation_commands.recv().await {
            let alive = match command {
                Command::Start if recording.is_none() => {
                    let client = dictation_authorization
                        .borrow()
                        .clone()
                        .ok_or_else(|| "Sign in to T3 before dictating.".to_owned())
                        .and_then(T3Client::new);
                    let ready = match client {
                        Ok(client) => client.voice_ready().await.map(|()| client),
                        Err(error) => Err(error),
                    };
                    match ready {
                        Ok(client) => {
                            let levels = dictation_updates.clone();
                            match Recorder::start(move |level| {
                                let _ = levels.send(Update::Dictation(Event::Level(level)));
                            })
                            .await
                            {
                                Ok((recorder, program)) => {
                                    recording = Some((recorder, client));
                                    send(Event::Recording { program })
                                }
                                Err(error) => send(Event::Finished(Err(error))),
                            }
                        }
                        Err(error) => send(Event::Finished(Err(error))),
                    }
                }
                Command::Stop => match recording.take() {
                    Some((recorder, client)) => {
                        send(Event::Transcribing);
                        let result = match recorder.stop().await {
                            Ok(pcm) => client.transcribe(&pcm).await,
                            Err(error) => Err(error),
                        };
                        send(Event::Finished(result))
                    }
                    None => true,
                },
                Command::Cancel => match recording.take() {
                    Some((recorder, _)) => {
                        recorder.cancel().await;
                        send(Event::Cancelled)
                    }
                    None => true,
                },
                Command::Start => true,
            };
            if !alive {
                break;
            }
        }
    });
    let thread_action_authorization = authorization.clone();
    let (thread_actions, mut thread_action_requests) =
        tokio::sync::mpsc::channel::<ThreadAction>(4);
    let thread_action_updates = sender.clone();
    runtime.spawn(async move {
        while let Some(action) = thread_action_requests.recv().await {
            let config = thread_action_authorization.borrow().clone();
            let (result, refreshed) = match config {
                Some(config) => match T3Client::new(config) {
                    Ok(client) => {
                        let result = client.run_thread_action(&action).await;
                        let refreshed = if result.is_ok() {
                            Some(client.catalog().await)
                        } else {
                            None
                        };
                        (result, refreshed)
                    }
                    Err(error) => (Err(error), None),
                },
                None => (Err("Sign in to T3 before modifying threads.".into()), None),
            };
            if thread_action_updates
                .send(Update::ThreadAction { action, result })
                .is_err()
            {
                break;
            }
            if let Some(catalog) = refreshed
                && thread_action_updates
                    .send(Update::Catalog(catalog))
                    .is_err()
            {
                break;
            }
        }
    });
    let send_authorization = authorization.clone();
    let (messages, mut outgoing) = tokio::sync::mpsc::channel::<crate::message::SendRequest>(1);
    let send_updates = sender.clone();
    runtime.spawn(async move {
        while let Some(request) = outgoing.recv().await {
            let config = send_authorization.borrow().clone();
            let result = match config {
                Some(config) => match T3Client::new(config) {
                    Ok(client) => client.send_message(&request).await,
                    Err(error) => Err(error.into()),
                },
                None => Err("Sign in to T3 before sending. Your draft is kept.".into()),
            };
            if send_updates.send(Update::Sent { request, result }).is_err() {
                break;
            }
        }
    });
    runtime.spawn(async move {
        let mut delay = 1_u64;
        loop {
            let selected = selection.borrow_and_update().clone();
            let config = authorization.borrow_and_update().clone();
            if let (Some(thread_id), Some(config)) = (selected, config) {
                let started = tokio::time::Instant::now();
                let stream = async {
                    let client = match T3Client::new(config) {
                        Ok(client) => client,
                        Err(error) => { let _ = history_sender.send(Update::Live { thread_id: thread_id.clone(), result: Err(error) }); return; }
                    };
                    if let Err(error) = client.stream_history(&thread_id, &history_sender).await {
                        // One HTTP resync on failure preserves readable history while reconnecting.
                        if let Ok(transcript) = client.transcript(&thread_id).await {
                            let _ = history_sender.send(Update::History { thread_id: thread_id.clone(), result: Ok(transcript) });
                        }
                        let _ = history_sender.send(Update::Live { thread_id: thread_id.clone(), result: Err(format!("{error} Reconnecting automatically.")) });
                    }
                };
                tokio::select! {
                    _ = stream => (),
                    changed = selection.changed() => { if changed.is_err() { break; } delay = 1; continue; }
                    changed = authorization.changed() => { if changed.is_err() { break; } delay = 1; continue; }
                }
                if started.elapsed() > Duration::from_secs(30) { delay = 1; }
            } else if let Some(thread_id) = selection.borrow().clone() {
                let _ = history_sender.send(Update::Live { thread_id, result: Err("Waiting for T3 connection and sign-in to resume live updates.".into()) });
            }
            tokio::select! {
                changed = selection.changed() => { if changed.is_err() { break; } delay = 1; }
                changed = authorization.changed() => { if changed.is_err() { break; } delay = 1; }
                _ = tokio::time::sleep(Duration::from_secs(delay)) => { delay = (delay * 2).min(15); }
            }
        }
    });
    runtime.spawn(async move {
        let mut client = loop {
            match Config::load().and_then(T3Client::new) {
                Ok(client) => break client,
                Err(error) => {
                    if sender.send(Update::Failed(error)).is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        };
        loop {
            let update = match client.check().await {
                Ok(connection) => Update::Ready(connection),
                Err(error) => {
                    if let Ok(config) = Config::load()
                        && config.origin != client.config.origin
                        && let Ok(reloaded) = T3Client::new(config)
                    {
                        client = reloaded;
                    }
                    Update::Failed(error)
                }
            };
            let authenticated =
                matches!(&update, Update::Ready(connection) if connection.authenticated);
            let config = authenticated.then(|| client.config.clone());
            if sender.send(update).is_err() {
                break;
            }
            credentials.send_if_modified(|saved| {
                if *saved == config {
                    false
                } else {
                    *saved = config;
                    true
                }
            });
            if authenticated
                && sender
                    .send(Update::Catalog(client.catalog().await))
                    .is_err()
            {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => (),
                request = requests.recv() => {
                    if request.is_none() { break; }
                    let update = match client.pair().await {
                        Ok(connection) => Update::Ready(connection),
                        Err(error) => Update::Pairing(error),
                    };
                    if sender.send(update).is_err() { break; }
                }
            }
        }
    });
    let (runtime_modes, mut runtime_mode_requests) =
        tokio::sync::mpsc::channel::<RuntimeModeChange>(4);
    runtime.spawn(async move {
        while let Some(request) = runtime_mode_requests.recv().await {
            let config = runtime_mode_authorization.borrow().clone();
            let result = match config
                .ok_or_else(|| "Sign in to T3 first.".to_owned())
                .and_then(T3Client::new)
            {
                Ok(client) => client.change_runtime_mode(&request).await,
                Err(error) => Err(error),
            };
            if runtime_mode_updates
                .send(Update::RuntimeModeChanged { request, result })
                .is_err()
            {
                break;
            }
        }
    });
    let (approvals, mut approval_requests) = tokio::sync::mpsc::channel::<ApprovalResponse>(4);
    runtime.spawn(async move {
        while let Some(request) = approval_requests.recv().await {
            let config = approval_authorization.borrow().clone();
            let result = match config
                .ok_or_else(|| "Sign in to T3 first.".to_owned())
                .and_then(T3Client::new)
            {
                Ok(client) => client.respond_approval(&request).await,
                Err(error) => Err(error),
            };
            if approval_updates
                .send(Update::ApprovalResponded { request, result })
                .is_err()
            {
                break;
            }
        }
    });
    let (server_updates, mut server_update_requests) =
        tokio::sync::mpsc::channel::<ServerCommand>(4);
    runtime.spawn(async move {
        while let Some(command) = server_update_requests.recv().await {
            let config = server_update_authorization.borrow().clone();
            let client = config
                .ok_or_else(|| "Sign in to T3 first.".to_owned())
                .and_then(T3Client::new);
            let update = match command {
                ServerCommand::CheckVersion { spec } => {
                    Update::ServerVersionChecked(match client {
                        Ok(client) => client.check_npm_release(&spec).await,
                        Err(error) => Err(error),
                    })
                }
                ServerCommand::StartUpdate {
                    target_version,
                    target_database_migration,
                } => Update::ServerUpdateFinished(match client {
                    Ok(client) => {
                        client
                            .update_server_with_progress(
                                &target_version,
                                target_database_migration,
                                &server_update_updates,
                            )
                            .await
                    }
                    Err(error) => Err(error),
                }),
            };
            if server_update_updates.send(update).is_err() {
                break;
            }
        }
    });
    Backend {
        editor,
        git,
        diffs,
        updates: receiver,
        pairing: commands,
        selected_thread,
        messages,
        models,
        runtime_modes,
        approvals,
        dictation,
        thread_actions,
        server_updates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    const DESCRIPTOR: &str = r#"{"environmentId":"test-id","label":"test-server","serverVersion":"0.1.8","orchestrationProtocolVersion":2}"#;

    fn fixture(
        responses: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (Config, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            for (expected, status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    assert_eq!(stream.read(&mut byte).unwrap(), 1);
                    request.push(byte[0]);
                }
                let headers = String::from_utf8(request).unwrap();
                assert!(headers.starts_with(expected));
                if expected.contains("/api/auth/session")
                    || expected.contains("/api/orchestration/")
                {
                    assert!(
                        headers
                            .to_lowercase()
                            .contains("authorization: bearer test-access")
                    );
                }
                if expected.contains("/api/orchestration/") {
                    assert!(
                        headers
                            .to_lowercase()
                            .contains("x-t3-orchestration-protocol: 2")
                    );
                }
                if let Some(length) = headers.lines().find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|n| n.parse::<usize>().ok())
                }) {
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).unwrap();
                    assert!(
                        String::from_utf8(body)
                            .unwrap()
                            .contains("subject_token=test-bootstrap")
                    );
                }
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (
            Config {
                origin,
                access_token: Some("test-access".into()),
                bootstrap_token: None,
            },
            worker,
        )
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)] // Tungstenite's handshake callback fixes this error type.
    async fn websocket_send_uses_authenticated_dispatch_and_requires_success_acknowledgement() {
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        for outcome in ["success", "failure", "closed"] {
            let success = outcome == "success";
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
            let request = crate::message::SendRequest::new("thread-a".into(), "Hello\n世界".into());
            let expected = request.clone();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_hdr_async(
                    stream,
                    |request: &Request, response: Response| {
                        assert_eq!(
                            request.uri().path_and_query().unwrap().as_str(),
                            "/ws?orchestrationProtocol=2"
                        );
                        assert_eq!(request.headers()["authorization"], "Bearer test-access");
                        Ok(response)
                    },
                )
                .await
                .unwrap();
                let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let value: serde_json::Value = serde_json::from_str(&message).unwrap();
                assert_eq!(value["tag"], "orchestration.dispatchCommand");
                assert_eq!(value["payload"], expected.payload());
                assert_eq!(
                    value["payload"]["dispatchMode"]["type"],
                    "queue_after_active"
                );
                socket
                    .send(Message::Text(r#"{"_tag":"Ping"}"#.into()))
                    .await
                    .unwrap();
                let pong = socket.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(pong.contains("Pong"));
                if outcome == "closed" {
                    socket.close(None).await.unwrap();
                    return;
                }
                let exit = if success {
                    serde_json::json!({"_tag":"Success","value":{"sequence":42}})
                } else {
                    serde_json::json!({"_tag":"Failure","cause":{"secret":"private prompt and credentials"}})
                };
                socket
                    .send(Message::Text(
                        serde_json::json!({"_tag":"Exit","requestId":"1","exit":exit})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                let _ = socket.next().await;
            });
            let config = Config {
                origin,
                access_token: Some("test-access".into()),
                bootstrap_token: None,
            };
            let result = T3Client::new(config).unwrap().send_message(&request).await;
            assert_eq!(result.is_ok(), success);
            if let Err(error) = result {
                assert!(!error.message.contains("private prompt"));
                assert_eq!(error.retry_same_id, outcome == "closed");
            }
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn thread_stream_acknowledges_chunks_and_delivers_partial_and_final_messages() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(request["tag"], "orchestration.subscribeThread");
            assert_eq!(request["payload"]["threadId"], "t1");
            assert!(request["payload"].get("acceptBoundedSnapshot").is_none());
            let values = vec![
                serde_json::json!({"kind":"snapshot","snapshotSequence":10,"projection":{"thread":{"id":"t1","title":"Chat"},"visibleTurnItems":[]}}),
                serde_json::json!({"kind":"event","sequence":11,"event":{"type":"turn-item.updated","payload":{"id":"a1","type":"assistant_message","text":"Hel","streaming":true}}}),
                serde_json::json!({"kind":"event","sequence":12,"event":{"type":"turn-item.updated","payload":{"id":"a1","type":"assistant_message","text":"Hello","streaming":false}}}),
            ];
            for value in values {
                socket
                    .send(Message::Text(
                        serde_json::json!({"_tag":"Chunk","requestId":"1","values":[value]})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                let ack: serde_json::Value = serde_json::from_str(
                    &socket.next().await.unwrap().unwrap().into_text().unwrap(),
                )
                .unwrap();
                assert_eq!(ack["_tag"], "Ack");
                assert_eq!(ack["requestId"], "1");
            }
            socket.close(None).await.unwrap();
        });
        let client = T3Client::new(Config {
            origin,
            access_token: Some("test-access".into()),
            bootstrap_token: None,
        })
        .unwrap();
        let (sender, updates) = mpsc::channel();
        let error = client.stream_history("t1", &sender).await.unwrap_err();
        assert!(error.contains("closed"));
        let received: Vec<_> = updates.try_iter().collect();
        assert_eq!(received.len(), 3);
        let mut app = crate::app::App {
            history_thread: Some("t1".into()),
            ..Default::default()
        };
        for update in received {
            let Update::Live { thread_id, result } = update else {
                panic!("unexpected update")
            };
            app.apply_live(&thread_id, result);
        }
        assert_eq!(app.transcript.as_ref().unwrap().items.len(), 1);
        let text = app.transcript.as_ref().unwrap().text();
        assert!(text.contains("Hello"));
        assert!(!text.contains("responding"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn model_change_dispatches_thread_setting_without_sending_a_message() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(request["tag"], "orchestration.dispatchCommand");
            assert_eq!(request["payload"]["type"], "thread.model-selection.set");
            assert_eq!(request["payload"]["threadId"], "t1");
            assert_eq!(
                request["payload"]["modelSelection"],
                serde_json::json!({"instanceId":"codex","model":"model-b"})
            );
            assert!(request["payload"].get("text").is_none());
            socket.send(Message::Text(r#"{"_tag":"Exit","requestId":"1","exit":{"_tag":"Success","value":{"sequence":2}}}"#.into())).await.unwrap();
            let _ = socket.next().await;
        });
        let client = T3Client::new(Config {
            origin,
            access_token: Some("test-access".into()),
            bootstrap_token: None,
        })
        .unwrap();
        let request = crate::models::ModelChange {
            thread_id: "t1".into(),
            command_id: "unique".into(),
            selection: crate::models::ModelSelection {
                instance_id: "codex".into(),
                model: "model-b".into(),
                options: None,
            },
        };
        client.change_model(&request).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a running, paired local T3 server; read-only"]
    async fn live_server_usage_subscription() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let catalog = client.catalog().await.unwrap();
        let (sender, updates) = mpsc::channel();
        let snapshot = async {
            loop {
                if let Ok(Update::Usage(Ok(providers))) = updates.try_recv() {
                    let count = providers.len();
                    let mut state = crate::usage::UsageState::default();
                    state.apply(providers);
                    let available = catalog
                        .threads
                        .iter()
                        .filter_map(|thread| thread.model_selection.as_ref())
                        .filter(|model| {
                            state
                                .remaining(&model.instance_id, &model.model, "session")
                                .is_some()
                                || state
                                    .remaining(&model.instance_id, &model.model, "weekly")
                                    .is_some()
                        })
                        .count();
                    println!(
                        "Received {count} providers; usage available for {available} listed threads."
                    );
                    assert!(state.loaded);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::select! {
            result = client.stream_usage(&sender) => panic!("usage stream ended before snapshot: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(10), snapshot) => result.unwrap(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a running, paired local T3 server; read-only"]
    async fn live_server_model_catalog() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let models = client.available_models().await.unwrap();
        assert!(!models.is_empty());
        let catalog = client.catalog().await.unwrap();
        assert!(
            catalog
                .threads
                .iter()
                .any(|thread| thread.model_selection.is_some())
        );
        println!("Available configured models: {}", models.len());
    }

    #[tokio::test]
    #[ignore = "requires a running T3 server paired with filesystem:read; reads status, branches, and working-tree diff"]
    async fn live_server_git_status() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let catalog = client.catalog().await.unwrap();
        let project = catalog
            .projects
            .iter()
            .find(|project| {
                project.workspace_root == std::env::current_dir().unwrap().to_string_lossy()
            })
            .unwrap();
        let request = crate::git::Request {
            id: "read-only-check".into(),
            cwd: project.workspace_root.clone(),
            operation: crate::git::Operation::Refresh,
        };
        let (sender, _) = mpsc::channel();
        let snapshot = client.git_request(&request, &sender).await.unwrap();
        assert!(snapshot.status.is_repo);
        assert!(!snapshot.branches.is_empty());
        if !snapshot.status.working_tree.files.is_empty() {
            let preview = client.rpc("review.getDiffPreview", serde_json::json!({
                "cwd":request.cwd,"sourceKind":"working-tree","trackedOnly":false,"metadataOnly":false,"ignoreWhitespace":false
            })).await.unwrap();
            assert!(
                preview["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|source| source["kind"] == "working-tree"
                        && source["diff"].as_str().is_some_and(|diff| !diff.is_empty()))
            );
        }
        println!(
            "Read-only Git status: {} local branches, {} changed files",
            snapshot.branches.len(),
            snapshot.status.working_tree.files.len()
        );
    }

    #[tokio::test]
    async fn git_commit_stream_acknowledges_progress_then_refreshes_repository() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            for method in ["git.runStackedAction", "vcs.refreshStatus", "vcs.listRefs"] {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let request: serde_json::Value = serde_json::from_str(
                    &socket.next().await.unwrap().unwrap().into_text().unwrap(),
                )
                .unwrap();
                assert_eq!(request["tag"], method);
                assert_eq!(request["payload"]["cwd"], "/worktree");
                let value = if method == "git.runStackedAction" {
                    assert_eq!(request["payload"]["action"], "commit");
                    assert_eq!(request["payload"]["commitMessage"], "Fix UI");
                    assert_eq!(request["payload"]["featureBranch"], false);
                    socket.send(Message::Text(serde_json::json!({"_tag":"Chunk","requestId":"1","values":[
                        {"kind":"phase_started","actionId":"test-action","label":"Committing"},
                        {"kind":"action_finished","actionId":"test-action"}
                    ]}).to_string().into())).await.unwrap();
                    let ack: serde_json::Value = serde_json::from_str(
                        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
                    )
                    .unwrap();
                    assert_eq!(ack["_tag"], "Ack");
                    serde_json::Value::Null
                } else if method == "vcs.refreshStatus" {
                    crate::git::tests::status()
                } else {
                    serde_json::json!({"refs":[{"name":"main"}],"nextCursor":null})
                };
                socket.send(Message::Text(serde_json::json!({"_tag":"Exit","requestId":"1","exit":{"_tag":"Success","value":value}}).to_string().into())).await.unwrap();
            }
        });
        let client = T3Client::new(Config {
            origin,
            access_token: Some("test-token".into()),
            bootstrap_token: None,
        })
        .unwrap();
        let request = crate::git::Request {
            id: "test-action".into(),
            cwd: "/worktree".into(),
            operation: crate::git::Operation::Commit("Fix UI".into()),
        };
        let (updates, received) = mpsc::channel();
        let snapshot = client.git_request(&request, &updates).await.unwrap();
        assert_eq!(snapshot.status.ref_name.as_deref(), Some("main"));
        assert!(matches!(
            received.try_recv().unwrap(),
            Update::GitProgress { .. }
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a running, paired local T3 server; read-only"]
    async fn live_server_checkpoint_diff() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let catalog = client.catalog().await.unwrap();
        for thread in &catalog.threads {
            let value = client
                .rpc(
                    "orchestration.getThreadProjection",
                    serde_json::json!({"threadId":thread.id}),
                )
                .await
                .unwrap();
            let projection = value.get("projection").unwrap_or(&value);
            let decoded: crate::history::Projection =
                serde_json::from_value(projection.clone()).unwrap();
            let ready = decoded
                .checkpoints
                .iter()
                .filter(|checkpoint| checkpoint.status == "ready")
                .count();
            println!("Decoded {ready} ready T3 checkpoint summaries");
            let Some(runs) = projection.get("runs").and_then(|runs| runs.as_array()) else {
                continue;
            };
            for run in runs
                .iter()
                .rev()
                .filter(|run| run["status"] == "completed")
                .take(10)
            {
                let Some(ordinal) = run["ordinal"].as_u64() else {
                    continue;
                };
                if let Ok(value) = client.rpc("orchestration.getTurnDiff", serde_json::json!({
                    "threadId":thread.id, "fromTurnCount":ordinal.saturating_sub(1), "toTurnCount":ordinal, "ignoreWhitespace":false
                })).await {
                    let diff = value["diff"].as_str().expect("server returns diff text");
                    let counts = crate::history::diff_counts(diff);
                    if !counts.is_empty() {
                        println!("Read-only checkpoint diff: {} files with line counts", counts.len());
                        return;
                    }
                }
            }
        }
        panic!("No nonempty completed checkpoint diff available for verification");
    }

    #[tokio::test]
    #[ignore = "requires a running, paired local T3 server; read-only"]
    async fn live_server_thread_subscription() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let catalog = client.catalog().await.unwrap();
        let thread_id = &catalog.threads.first().expect("server needs a thread").id;
        let (sender, updates) = mpsc::channel();
        let snapshot = async {
            loop {
                if let Ok(Update::Live {
                    result: Ok(crate::history::LiveUpdate::Snapshot { projection, .. }),
                    ..
                }) = updates.try_recv()
                {
                    assert_eq!(&projection.thread.id, thread_id);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::select! {
            result = client.stream_history(thread_id, &sender) => panic!("subscription ended before snapshot: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(10), snapshot) => result.unwrap(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a running, paired local T3 server; read-only"]
    async fn live_server_websocket_authentication() {
        let mut client = T3Client::new(Config::load().unwrap()).unwrap();
        assert!(client.check().await.unwrap().authenticated);
        let result = client
            .rpc(
                "orchestration.getThreadTitles",
                serde_json::json!({"threadIds":[]}),
            )
            .await
            .unwrap();
        assert!(result.is_object());
    }

    #[tokio::test]
    async fn session_rejection_is_reported_as_signed_out_rather_than_network_failure() {
        for status in ["401 Unauthorized", "403 Forbidden"] {
            let (config, worker) = fixture(vec![
                ("GET /.well-known/t3/environment ", "200 OK", DESCRIPTOR),
                ("GET /api/auth/session ", status, "private error detail"),
            ]);
            assert!(
                !T3Client::new(config)
                    .unwrap()
                    .check()
                    .await
                    .unwrap()
                    .authenticated
            );
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn loads_encoded_thread_transcript_and_rejects_mismatched_identity() {
        for (thread_id, path) in [
            (
                "t/odd?id",
                "GET /api/orchestration/threads/t%2Fodd%3Fid/transcript ",
            ),
            ("other", "GET /api/orchestration/threads/other/transcript "),
        ] {
            let (config, worker) = fixture(vec![(
                path,
                "200 OK",
                r#"{"threadId":"t/odd?id","title":"Test chat","items":[]}"#,
            )]);
            let result = T3Client::new(config).unwrap().transcript(thread_id).await;
            assert_eq!(result.is_ok(), thread_id == "t/odd?id");
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn loads_catalog_with_authentication_and_protocol_header() {
        let (config, worker) = fixture(vec![(
            "GET /api/orchestration/shell ",
            "200 OK",
            r#"{"projects":[{"id":"p1","title":"Project","workspaceRoot":"/project"}],"threads":[{"id":"t1","projectId":"p1","title":"Chat","status":"idle"}],"archivedThreads":[]}"#,
        )]);
        let catalog = T3Client::new(config).unwrap().catalog().await.unwrap();
        assert_eq!(catalog.projects.len(), 1);
        assert_eq!(catalog.threads.len(), 1);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn verifies_identity_and_bearer_session() {
        let (config, worker) = fixture(vec![
            ("GET /.well-known/t3/environment ", "200 OK", DESCRIPTOR),
            (
                "GET /api/auth/session ",
                "200 OK",
                r#"{"authenticated":true}"#,
            ),
        ]);
        let connection = T3Client::new(config).unwrap().check().await.unwrap();
        assert!(connection.authenticated);
        assert_eq!(connection.environment.label, "test-server");
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn exchanges_bootstrap_and_reports_missing_auth() {
        let (mut config, worker) = fixture(vec![
            ("GET /.well-known/t3/environment ", "200 OK", DESCRIPTOR),
            (
                "POST /oauth/token ",
                "200 OK",
                r#"{"access_token":"test-access","token_type":"Bearer"}"#,
            ),
            (
                "GET /api/auth/session ",
                "200 OK",
                r#"{"authenticated":false}"#,
            ),
        ]);
        config.access_token = None;
        config.bootstrap_token = Some("test-bootstrap".into());
        assert!(
            !T3Client::new(config)
                .unwrap()
                .check()
                .await
                .unwrap()
                .authenticated
        );
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn restores_saved_session_and_detects_revocation() {
        for body in [r#"{"authenticated":true}"#, r#"{"authenticated":false}"#] {
            let (mut config, worker) = fixture(vec![
                ("GET /.well-known/t3/environment ", "200 OK", DESCRIPTOR),
                ("GET /api/auth/session ", "200 OK", body),
            ]);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("tui3/session.json");
            crate::pairing::save(&path, &config.origin, "test-id", "test-access").unwrap();
            config.access_token = None;
            let mut client = T3Client::new(config).unwrap();
            client.session_path = Some(path);
            let connection = client.check().await.unwrap();
            assert_eq!(connection.authenticated, body.contains("true"));
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_bad_responses_without_exposing_body() {
        for (status, body) in [("401 Unauthorized", "secret-value"), ("200 OK", "not-json")] {
            let (config, worker) =
                fixture(vec![("GET /.well-known/t3/environment ", status, body)]);
            let error = T3Client::new(config).unwrap().check().await.err().unwrap();
            assert!(!error.contains(body));
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn check_npm_release_and_update_stream_handle_progress_and_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            // 1. server.getNpmRelease
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let req: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(req["tag"], "server.getNpmRelease");
            assert_eq!(req["payload"]["spec"], "latest");
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "_tag": "Exit",
                        "requestId": "1",
                        "exit": {
                            "_tag": "Success",
                            "value": { "version": "0.1.11", "databaseMigration": 4 }
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();

            // 2. server.updateServerWithProgress (success)
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let req: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(req["tag"], "server.updateServerWithProgress");
            assert_eq!(req["payload"]["targetVersion"], "0.1.11");
            assert_eq!(req["payload"]["targetDatabaseMigration"], 4);
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "_tag": "Chunk",
                        "requestId": "1",
                        "values": [{ "type": "progress", "stage": "downloading" }]
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let ack: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(ack["_tag"], "Ack");
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "_tag": "Chunk",
                        "requestId": "1",
                        "values": [
                            { "type": "progress", "stage": "installing" },
                            { "type": "complete", "result": { "targetVersion": "0.1.11", "method": "boot-service" } }
                        ]
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let ack: serde_json::Value =
                serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(ack["_tag"], "Ack");
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "_tag": "Exit",
                        "requestId": "1",
                        "exit": { "_tag": "Success", "value": null }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();

            // 3. server.updateServerWithProgress (failure reason)
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = socket.next().await;
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "_tag": "Exit",
                        "requestId": "1",
                        "exit": {
                            "_tag": "Failure",
                            "cause": {
                                "failure": {
                                    "reason": "Update the desktop app to update this server."
                                }
                            }
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let config = Config {
            origin,
            access_token: Some("test-access".into()),
            bootstrap_token: None,
        };
        let client = T3Client::new(config).unwrap();
        let release = client.check_npm_release("latest").await.unwrap().unwrap();
        assert_eq!(release.version, "0.1.11");
        assert_eq!(release.database_migration, Some(4));

        let (tx, rx) = mpsc::channel();
        let updated = client
            .update_server_with_progress("0.1.11", Some(4), &tx)
            .await
            .unwrap();
        assert_eq!(updated, "0.1.11");
        let events: Vec<_> = rx.try_iter().collect();
        assert!(matches!(
            events.as_slice(),
            [
                Update::ServerUpdateProgress(ServerUpdateStage::Downloading),
                Update::ServerUpdateProgress(ServerUpdateStage::Installing),
            ]
        ));

        let err = client
            .update_server_with_progress("0.1.11", Some(4), &tx)
            .await
            .unwrap_err();
        assert_eq!(err, "Update the desktop app to update this server.");
        server.await.unwrap();
    }
}
