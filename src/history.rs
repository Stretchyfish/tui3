use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub thread_id: String,
    pub title: String,
    pub items: Vec<ProjectedItem>,
}

#[derive(Clone, Deserialize)]
pub struct ProjectedItem {
    pub position: u64,
    #[serde(rename = "sourceItemId")]
    pub source_item_id: Option<String>,
    pub item: Item,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalOption {
    pub decision: String,
    pub label: String,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequest {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub decision: Option<String>,
    pub feedback: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct Item {
    #[serde(rename = "messageId")]
    pub message_id: Option<String>,
    #[serde(rename = "fileName")]
    pub file_name: Option<String>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "inputIntent")]
    pub input_intent: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
    #[serde(rename = "requestKind")]
    pub request_kind: Option<String>,
    pub prompt: Option<String>,
    pub reason: Option<String>,
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
    #[serde(default)]
    pub options: Vec<ApprovalOption>,
}

impl Transcript {
    pub fn messages(&self) -> Vec<&ProjectedItem> {
        let mut messages: Vec<_> = self
            .items
            .iter()
            .filter(|entry| {
                matches!(
                    entry.item.kind.as_str(),
                    "user_message" | "assistant_message" | "system_message" | "approval_request"
                )
            })
            .collect();
        messages.sort_by_key(|entry| entry.position);
        messages
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        let messages = self.messages();
        if messages.is_empty() {
            return "No chat messages in this thread yet.".into();
        }
        let mut text = String::new();
        for entry in messages {
            let item = &entry.item;
            let role = match item.kind.as_str() {
                "user_message" => "You",
                "assistant_message" => "Assistant",
                _ => "System",
            };
            text.push_str(role);
            if item.streaming {
                text.push_str(" (responding…)");
            }
            text.push('\n');
            text.push_str(item.text.as_deref().unwrap_or_default());
            if !item.attachments.is_empty() {
                text.push_str(&format!(
                    "\n[{} attachment(s); preview unavailable]",
                    item.attachments.len()
                ));
            }
            text.push_str("\n\n");
        }
        text
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn checkpoint_diff_counts_include_deleted_files_and_multiple_hunks() {
        let counts = diff_counts(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n@@ -5 +5,2 @@\n same\n+extra\ndiff --git a/gone.rs b/gone.rs\n--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n",
        );
        assert_eq!(counts.get("a.rs"), Some(&(2, 1)));
        assert_eq!(counts.get("gone.rs"), Some(&(0, 1)));
    }

    pub fn transcript(id: &str) -> Transcript {
        let json = r#"{"threadId":"ID","title":"Conversation","items":[
            {"position":3,"item":{"type":"assistant_message","text":"Hello, 世界!","streaming":true}},
            {"position":2,"item":{"type":"command_execution","input":"hidden tool log"}},
            {"position":1,"item":{"type":"user_message","text":"Question","attachments":[{"type":"file"}]}},
            {"position":4,"item":{"type":"future_activity","data":{}}}
        ]}"#;
        serde_json::from_str(&json.replace("ID", id)).unwrap()
    }

    #[test]
    fn displays_ordered_conversation_with_unicode_and_attachment_markers() {
        let text = transcript("t1").text();
        assert!(text.find("Question").unwrap() < text.find("Hello, 世界!").unwrap());
        assert!(text.contains("attachment(s)"));
        assert!(text.contains("responding…"));
        assert!(!text.contains("hidden tool log"));
        let mut empty = transcript("t1");
        empty.items.clear();
        assert!(empty.text().contains("No chat messages"));
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind")]
pub enum LiveUpdate {
    #[serde(rename = "snapshot")]
    Snapshot {
        #[serde(rename = "snapshotSequence")]
        sequence: u64,
        projection: Projection,
    },
    #[serde(rename = "event")]
    Event { sequence: u64, event: DomainEvent },
    #[serde(rename = "synchronized")]
    Synchronized,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    #[serde(default)]
    pub messages: Vec<ConversationMessage>,
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default)]
    pub runtime_requests: Vec<RuntimeRequest>,
    pub thread: ThreadIdentity,
    pub visible_turn_items: Vec<ProjectedItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub files_total: Option<CheckpointTotals>,
    pub run_id: Option<String>,
    pub status: String,
    pub files: Vec<CheckpointFile>,
}

#[derive(Deserialize)]
pub struct CheckpointTotals {
    pub count: usize,
}

#[derive(Deserialize)]
pub struct CheckpointFile {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    pub run_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub user_message_id: Option<String>,
    pub queue_position: Option<u64>,
    pub ordinal: Option<u64>,
    pub id: String,
    pub status: String,
    pub requested_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

pub type FileCounts = std::collections::BTreeMap<String, (u64, u64)>;

pub fn diff_counts(diff: &str) -> FileCounts {
    let mut files = FileCounts::new();
    let mut old_path = None;
    let mut path = None;
    let mut hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            hunk = false;
            path = None;
        } else if !hunk && line.starts_with("--- ") {
            old_path = Some(line[4..].trim_start_matches("a/").to_owned());
        } else if !hunk && line.starts_with("+++ ") {
            let name = line[4..].trim_start_matches("b/");
            path = if name == "/dev/null" {
                old_path.clone()
            } else {
                Some(name.to_owned())
            };
            if let Some(path) = &path {
                files.entry(path.clone()).or_insert((0, 0));
            }
        } else if line.starts_with("@@ ") {
            hunk = true;
        } else if hunk && let Some(counts) = path.as_ref().and_then(|path| files.get_mut(path)) {
            if line.starts_with('+') {
                counts.0 += 1;
            } else if line.starts_with('-') {
                counts.1 += 1;
            }
        }
    }
    files
}

impl Run {
    pub fn active(&self) -> bool {
        matches!(
            self.status.as_str(),
            "preparing" | "queued" | "starting" | "running" | "waiting"
        )
    }

    pub fn duration(&self) -> Option<std::time::Duration> {
        let start = chrono::DateTime::parse_from_rfc3339(
            self.started_at.as_ref().or(self.requested_at.as_ref())?,
        )
        .ok()?;
        let end = chrono::DateTime::parse_from_rfc3339(self.completed_at.as_ref()?).ok()?;
        (end - start).to_std().ok()
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub total_processed_tokens: Option<u64>,
    pub max_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub last_used_tokens: Option<u64>,
    pub last_input_tokens: Option<u64>,
    pub last_cached_input_tokens: Option<u64>,
    pub last_output_tokens: Option<u64>,
    pub last_reasoning_output_tokens: Option<u64>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
    pub compacts_automatically: Option<bool>,
    pub auto_compact_threshold: Option<u64>,
    pub cost: Option<ContextCost>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCost {
    pub amount: f64,
    pub currency: String,
}

#[derive(Deserialize)]
pub struct ThreadIdentity {
    pub id: String,
    pub title: String,
    #[serde(rename = "modelSelection")]
    pub model_selection: Option<crate::models::ModelSelection>,
    #[serde(default, rename = "runtimeMode")]
    pub runtime_mode: Option<String>,
    #[serde(default, rename = "contextUsage")]
    pub context_usage: Option<ContextUsage>,
}

#[derive(Deserialize)]
pub struct DomainEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl Transcript {
    pub fn upsert(&mut self, item: Item) {
        let Some(id) = &item.id else {
            return;
        };
        if let Some(entry) = self.items.iter_mut().find(|entry| {
            entry.item.id.as_ref() == Some(id) || entry.source_item_id.as_ref() == Some(id)
        }) {
            entry.item = item;
        } else {
            let position = self
                .items
                .iter()
                .map(|entry| entry.position)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            self.items.push(ProjectedItem {
                position,
                source_item_id: item.id.clone(),
                item,
            });
        }
    }
}
