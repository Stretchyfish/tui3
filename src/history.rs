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

#[derive(Clone, Deserialize)]
pub struct Item {
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
}

impl Transcript {
    pub fn messages(&self) -> Vec<&ProjectedItem> {
        let mut messages: Vec<_> = self
            .items
            .iter()
            .filter(|entry| {
                matches!(
                    entry.item.kind.as_str(),
                    "user_message" | "assistant_message" | "system_message"
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
    pub runs: Vec<Run>,
    pub thread: ThreadIdentity,
    pub visible_turn_items: Vec<ProjectedItem>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub status: String,
    pub requested_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
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

#[derive(Deserialize)]
pub struct ThreadIdentity {
    pub id: String,
    pub title: String,
    #[serde(rename = "modelSelection")]
    pub model_selection: Option<crate::models::ModelSelection>,
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
