use crate::catalog::{Catalog, Row, Selection};
use crate::message::SendRequest;
use ratatui::widgets::ListState;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ActivityNote {
    pub id: String,
    pub summary: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Projects,
    Chat,
    Composer,
    Model,
    Workspace,
    WorkspaceTabs,
}

#[derive(Clone, Copy)]
pub enum ClickTarget {
    DiffBody,
    DiffFile(usize),
    Pane(Focus),
    Workspace(usize),
    TypingStyle(crate::preferences::TypingStyle),
}

pub struct App {
    pub chat_revision: u64,
    pub chat_cache: Option<(u64, Option<u64>, ratatui::text::Text<'static>)>,
    pub chat_viewport: Option<crate::chat::Viewport>,
    pub diff_view: crate::diff::DiffView,
    pub run_diffs: HashMap<String, crate::history::FileCounts>,
    pub diff_requested: std::collections::HashSet<String>,
    pub click_targets: Vec<(ratatui::layout::Rect, ClickTarget)>,
    pub workspace_tab: usize,
    pub workspace_highlight: usize,
    pub style_highlight: crate::preferences::TypingStyle,
    pub settings_open: bool,
    pub typing_style: crate::preferences::TypingStyle,
    pub focus: Focus,
    pub models: crate::models::ModelPicker,
    pub usage: crate::usage::UsageState,
    pub live_model: Option<crate::models::ModelSelection>,
    pub draft: String,
    pub draft_cursor: usize,
    pub editing: bool,
    pub quit: bool,
    pub connection: String,
    pub authenticated: bool,
    pub pairing_requested: bool,
    pub pairing_in_progress: bool,
    pub pairing_message: String,
    pub catalog: Option<Catalog>,
    pub restore_location: Option<crate::preferences::Location>,
    pub rows: Vec<Row>,
    pub sidebar_state: ListState,
    pub catalog_error: Option<String>,
    pub history_thread: Option<String>,
    pub transcript: Option<crate::history::Transcript>,
    pub history_error: Option<String>,
    pub history_sequence: Option<u64>,
    pub history_live: bool,
    pub activity: Vec<ActivityNote>,
    pub thinking_since: Option<Instant>,
    pub worked_duration: Option<Duration>,
    pub runs: HashMap<String, crate::history::Run>,
    pub chat_scroll: u16,
    pub chat_max_scroll: u16,
    pub follow_chat: bool,
    pub send_requested: bool,
    pub send_pending: Option<SendRequest>,
    pub send_notice: Option<String>,
    pub send_status: HashMap<String, String>,
    pub last_attempt: HashMap<String, SendRequest>,
    pub draft_thread: Option<String>,
    pub drafts: HashMap<Option<String>, String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            chat_revision: 0,
            chat_cache: None,
            chat_viewport: None,
            diff_view: Default::default(),
            run_diffs: HashMap::new(),
            diff_requested: Default::default(),
            click_targets: Vec::new(),
            workspace_tab: 0,
            workspace_highlight: 0,
            style_highlight: crate::preferences::TypingStyle::default(),
            settings_open: false,
            typing_style: crate::preferences::TypingStyle::default(),
            focus: Focus::default(),
            models: crate::models::ModelPicker::default(),
            usage: crate::usage::UsageState::default(),
            live_model: None,
            draft: String::new(),
            draft_cursor: 0,
            editing: false,
            quit: false,
            connection: "Connecting to T3…".into(),
            authenticated: false,
            pairing_requested: false,
            pairing_in_progress: false,
            pairing_message: "Press p to pair with your local T3 server. No tokens to copy.".into(),
            catalog: None,
            restore_location: None,
            rows: Vec::new(),
            sidebar_state: ListState::default(),
            catalog_error: None,
            history_thread: None,
            transcript: None,
            history_error: None,
            history_sequence: None,
            history_live: false,
            activity: Vec::new(),
            thinking_since: None,
            worked_duration: None,
            runs: HashMap::new(),
            chat_scroll: 0,
            chat_max_scroll: 0,
            follow_chat: true,
            send_requested: false,
            send_pending: None,
            send_notice: None,
            send_status: HashMap::new(),
            last_attempt: HashMap::new(),
            draft_thread: None,
            drafts: HashMap::new(),
        }
    }
}

impl App {
    fn apply_checkpoint(&mut self, checkpoint: crate::history::Checkpoint) {
        if checkpoint.status != "ready" {
            return;
        }
        if checkpoint
            .files_total
            .is_some_and(|total| total.count > checkpoint.files.len())
        {
            return;
        }
        if let Some(run) = checkpoint.run_id {
            self.run_diffs.insert(
                run.clone(),
                checkpoint
                    .files
                    .into_iter()
                    .map(|file| (file.path, (file.additions, file.deletions)))
                    .collect(),
            );
        }
    }
    fn record_activity(&mut self, id: String, summary: String) {
        if let Some(index) = self.activity.iter().position(|entry| entry.id == id) {
            self.activity.remove(index);
        }
        self.activity.push(ActivityNote { id, summary });
        if self.activity.len() > 6 {
            self.activity.remove(0);
        }
    }

    fn record_item_activity(&mut self, item: &crate::history::Item) {
        let id = item
            .id
            .clone()
            .unwrap_or_else(|| format!("{}:activity", item.kind));
        self.activity.retain(|entry| entry.id != id);
        let Some(summary) = item_activity_summary(item) else {
            return;
        };
        self.record_activity(id, summary);
    }

    fn start_work_timer(&mut self) {
        if self.thinking_since.is_none() {
            self.thinking_since = Some(Instant::now());
            self.worked_duration = None;
        }
    }

    fn stop_work_timer(&mut self) {
        if let Some(started) = self.thinking_since.take() {
            self.worked_duration = Some(started.elapsed());
        }
    }

    fn draft_byte_cursor(&mut self) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        let target = self.draft_cursor;
        let mut chars = 0;
        for (byte, grapheme) in self.draft.grapheme_indices(true) {
            let next = chars + grapheme.chars().count();
            if target < next {
                self.draft_cursor = chars;
                return byte;
            }
            chars = next;
        }
        self.draft_cursor = chars;
        self.draft.len()
    }

    pub fn insert_draft(&mut self, text: &str) {
        let byte = self.draft_byte_cursor();
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let text: String = text
            .chars()
            .filter(|ch| *ch == '\n' || !ch.is_control())
            .collect();
        self.draft.insert_str(byte, &text);
        self.draft_cursor += text.chars().count();
    }

    pub fn move_draft_word(&mut self, right: bool) {
        use unicode_segmentation::UnicodeSegmentation;
        let byte = self.draft_byte_cursor();
        let destination = if right {
            self.draft
                .unicode_word_indices()
                .find(|(start, _)| *start > byte)
                .map_or(self.draft.len(), |(start, _)| start)
        } else {
            self.draft
                .unicode_word_indices()
                .rev()
                .find(|(start, _)| *start < byte)
                .map_or(0, |(start, _)| start)
        };
        self.draft_cursor = self.draft[..destination].chars().count();
    }

    pub fn backspace_draft(&mut self) {
        use unicode_segmentation::UnicodeSegmentation;
        let end = self.draft_byte_cursor();
        if let Some((start, _)) = self.draft[..end].grapheme_indices(true).next_back() {
            let removed = self.draft[start..end].chars().count();
            self.draft.drain(start..end);
            self.draft_cursor -= removed;
        }
    }

    pub fn delete_draft(&mut self) {
        use unicode_segmentation::UnicodeSegmentation;
        let start = self.draft_byte_cursor();
        if let Some(grapheme) = self.draft[start..].graphemes(true).next() {
            let end = start + grapheme.len();
            self.draft.drain(start..end);
        }
    }

    pub fn move_draft_cursor(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        use unicode_segmentation::UnicodeSegmentation;
        let byte = self.draft_byte_cursor();
        match code {
            KeyCode::Left => {
                if let Some(grapheme) = self.draft[..byte].graphemes(true).next_back() {
                    self.draft_cursor -= grapheme.chars().count();
                }
            }
            KeyCode::Right => {
                if let Some(grapheme) = self.draft[byte..].graphemes(true).next() {
                    self.draft_cursor += grapheme.chars().count();
                }
            }
            KeyCode::Home => {
                let before = self
                    .draft
                    .chars()
                    .take(self.draft_cursor)
                    .collect::<String>();
                self.draft_cursor = before
                    .rfind('\n')
                    .map_or(0, |i| before[..i].chars().count() + 1);
            }
            KeyCode::End => {
                let after = self
                    .draft
                    .chars()
                    .skip(self.draft_cursor)
                    .collect::<String>();
                self.draft_cursor += after
                    .find('\n')
                    .map_or(after.chars().count(), |i| after[..i].chars().count());
            }
            KeyCode::Up | KeyCode::Down => {
                let before = self
                    .draft
                    .chars()
                    .take(self.draft_cursor)
                    .collect::<String>();
                let line_start = before
                    .rfind('\n')
                    .map_or(0, |i| before[..i].chars().count() + 1);
                let col = self.draft_cursor - line_start;
                if code == KeyCode::Up && line_start > 0 {
                    let prev_end = line_start - 1;
                    let prev_start = before[..before.rfind('\n').unwrap_or(0)]
                        .rfind('\n')
                        .map_or(0, |i| before[..i].chars().count() + 1);
                    self.draft_cursor = prev_start + col.min(prev_end - prev_start);
                } else if code == KeyCode::Down {
                    let rest = self
                        .draft
                        .chars()
                        .skip(self.draft_cursor)
                        .collect::<String>();
                    if let Some(nl) = rest.find('\n') {
                        let next_start = self.draft_cursor + rest[..nl].chars().count() + 1;
                        let remaining = self.draft.chars().skip(next_start).collect::<String>();
                        self.draft_cursor = next_start
                            + col.min(
                                remaining.find('\n').map_or(remaining.chars().count(), |i| {
                                    remaining[..i].chars().count()
                                }),
                            );
                    }
                }
            }
            _ => (),
        }
    }

    pub fn selected_thread_id(&self) -> Option<&str> {
        match self.selection() {
            Some(Selection::Thread(id)) => Some(id),
            _ => None,
        }
    }

    pub fn current_model(&self) -> Option<&crate::models::ModelSelection> {
        let id = self.selected_thread_id()?;
        if self.history_thread.as_deref() == Some(id) && self.live_model.is_some() {
            return self.live_model.as_ref();
        }
        self.catalog
            .as_ref()?
            .threads
            .iter()
            .find(|thread| thread.id == id)?
            .model_selection
            .as_ref()
    }

    pub fn set_thread_model(&mut self, id: &str, selection: crate::models::ModelSelection) {
        if self.selected_thread_id() == Some(id) {
            self.live_model = Some(selection.clone());
        }
        if let Some(catalog) = &mut self.catalog
            && let Some(thread) = catalog.threads.iter_mut().find(|thread| thread.id == id)
        {
            thread.model_selection = Some(selection);
        }
    }

    pub fn open_models(&mut self) {
        self.models.notice_thread = self.selected_thread_id().map(str::to_owned);
        if self.models.pending.is_some() {
            self.models.notice = Some("Wait for the current model change.".into());
            return;
        }
        if !self.authenticated || self.selected_thread_id().is_none() {
            self.models.notice = Some("Select a thread and sign in first.".into());
            return;
        }
        self.models.thread_id = self.selected_thread_id().map(str::to_owned);
        self.models.open = true;
        self.models.notice = None;
        self.models.error = None;
        self.select_current_model();
        if self.models.choices.is_empty() && !self.models.loading {
            self.request_models();
        }
    }

    pub fn request_models(&mut self) {
        self.models.loading = true;
        self.models.load_requested = true;
        self.models.error = None;
    }

    pub fn select_current_model(&mut self) {
        let index = self.current_model().and_then(|current| {
            self.models.choices.iter().position(|choice| {
                choice.selection.instance_id == current.instance_id
                    && choice.selection.model == current.model
            })
        });
        self.models
            .list_state
            .select(index.or_else(|| (!self.models.choices.is_empty()).then_some(0)));
    }

    pub fn move_model(&mut self, down: bool, amount: usize) {
        if self.models.choices.is_empty() {
            return;
        }
        let current = self.models.list_state.selected().unwrap_or(0);
        self.models.list_state.select(Some(if down {
            current
                .saturating_add(amount)
                .min(self.models.choices.len() - 1)
        } else {
            current.saturating_sub(amount)
        }));
    }

    pub fn prepare_model_change(&mut self) -> Option<crate::models::ModelChange> {
        if self.models.loading || self.models.pending.is_some() {
            return None;
        }
        if self.models.thread_id.as_deref() != self.selected_thread_id() || !self.authenticated {
            self.models.error = Some("Thread selection changed. Reopen the selector.".into());
            return None;
        }
        if self.send_pending.is_some() {
            self.models.error =
                Some("Wait until the message is sent before changing model.".into());
            return None;
        }
        let selection = self
            .models
            .choices
            .get(self.models.list_state.selected()?)?
            .selection
            .clone();
        self.models.open = false;
        if self.current_model().is_some_and(|current| {
            current.instance_id == selection.instance_id && current.model == selection.model
        }) {
            return None;
        }
        let request = crate::models::ModelChange {
            thread_id: self.selected_thread_id()?.into(),
            command_id: uuid::Uuid::new_v4().to_string(),
            selection,
        };
        self.models.pending = Some(request.clone());
        self.models.notice_thread = Some(request.thread_id.clone());
        self.models.notice = Some("Changing model…".into());
        Some(request)
    }

    pub fn finish_model_change(
        &mut self,
        request: crate::models::ModelChange,
        result: Result<(), String>,
    ) {
        if self
            .models
            .pending
            .as_ref()
            .map(|pending| &pending.command_id)
            != Some(&request.command_id)
        {
            return;
        }
        self.models.pending = None;
        self.models.notice_thread = Some(request.thread_id.clone());
        match result {
            Ok(()) => {
                self.set_thread_model(&request.thread_id, request.selection);
                self.models.notice = (self.selected_thread_id() == Some(&request.thread_id))
                    .then(|| "Model updated for future messages.".into());
            }
            Err(error) => self.models.notice = Some(error),
        }
    }

    pub fn prepare_send(&mut self) -> Option<SendRequest> {
        self.send_notice = None;
        let id = match self.selection() {
            Some(Selection::Thread(id)) if self.authenticated => id.clone(),
            _ => {
                self.send_notice = Some("Select a thread and sign in before sending.".into());
                return None;
            }
        };
        if self.models.pending.is_some() {
            self.send_notice = Some("Wait for the model change before sending.".into());
            return None;
        }
        if self.send_pending.is_some() {
            self.send_notice = Some("A message is still being sent. Please wait.".into());
            return None;
        }
        if self.draft.trim().is_empty() {
            self.send_notice = Some("Type a message before sending.".into());
            return None;
        }
        if self.draft_thread.as_deref() != Some(&id) {
            self.send_notice = Some("Thread selection changed. Please try again.".into());
            return None;
        }
        let request = self
            .last_attempt
            .get(&id)
            .filter(|request| request.text == self.draft)
            .cloned()
            .unwrap_or_else(|| SendRequest::new(id.clone(), self.draft.clone()));
        self.last_attempt.insert(id.clone(), request.clone());
        self.send_status.insert(id, "Sending…".into());
        self.send_pending = Some(request.clone());
        Some(request)
    }

    pub fn finish_send(
        &mut self,
        request: SendRequest,
        result: Result<(), crate::message::SendFailure>,
    ) {
        if self
            .send_pending
            .as_ref()
            .map(|pending| &pending.command_id)
            != Some(&request.command_id)
        {
            return;
        }
        self.send_pending = None;
        if self.draft_thread.as_deref() == Some(&request.thread_id) {
            self.send_notice = None;
        }
        match result {
            Ok(()) => {
                if self.draft_thread.as_deref() == Some(&request.thread_id) {
                    if self.draft == request.text {
                        self.draft.clear();
                        self.draft_cursor = 0;
                    }
                    self.follow_chat = true;
                } else if self.drafts.get(&Some(request.thread_id.clone())) == Some(&request.text) {
                    self.drafts.remove(&Some(request.thread_id.clone()));
                }
                self.last_attempt.remove(&request.thread_id);
                self.send_status.insert(
                    request.thread_id,
                    "Sent to T3. Queued after any active run.".into(),
                );
            }
            Err(error) => {
                if !error.retry_same_id {
                    self.last_attempt.remove(&request.thread_id);
                }
                self.send_status
                    .insert(request.thread_id, format!("Send failed: {}", error.message));
            }
        }
    }

    pub fn sync_history_selection(&mut self) -> bool {
        let draft_thread = match self.selection() {
            Some(Selection::Thread(id)) => Some(id.clone()),
            _ => None,
        };
        if self.draft_thread != draft_thread {
            self.drafts
                .insert(self.draft_thread.clone(), std::mem::take(&mut self.draft));
            self.draft = self.drafts.remove(&draft_thread).unwrap_or_default();
            self.draft_cursor = self.draft.chars().count();
            self.draft_thread = draft_thread;
            self.send_notice = None;
        }
        let selected = match self.selection() {
            Some(Selection::Thread(id)) if self.authenticated => Some(id.clone()),
            _ => None,
        };
        if selected == self.history_thread {
            return false;
        }
        self.history_thread = selected;
        self.live_model = None;
        self.history_sequence = None;
        self.history_live = false;
        self.activity.clear();
        self.thinking_since = None;
        self.worked_duration = None;
        self.runs.clear();
        self.run_diffs.clear();
        self.diff_view = Default::default();
        self.diff_requested.clear();
        self.transcript = None;
        self.chat_revision = self.chat_revision.wrapping_add(1);
        self.history_error = None;
        self.chat_scroll = 0;
        self.chat_max_scroll = 0;
        self.follow_chat = true;
        true
    }

    pub fn apply_live(
        &mut self,
        thread_id: &str,
        result: Result<crate::history::LiveUpdate, String>,
    ) {
        use crate::history::LiveUpdate;
        if self.history_thread.as_deref() != Some(thread_id) {
            return;
        }
        self.chat_revision = self.chat_revision.wrapping_add(1);
        let update = match result {
            Ok(update) => update,
            Err(error) => {
                self.history_error = Some(error);
                self.history_live = false;
                return;
            }
        };
        match update {
            LiveUpdate::Snapshot {
                sequence,
                projection,
            } => {
                if projection.thread.id != thread_id {
                    return;
                }
                self.live_model = projection.thread.model_selection;
                for checkpoint in projection.checkpoints {
                    self.apply_checkpoint(checkpoint);
                }
                self.runs = projection
                    .runs
                    .into_iter()
                    .map(|run| (run.id.clone(), run))
                    .collect();
                self.transcript = Some(crate::history::Transcript {
                    thread_id: projection.thread.id,
                    title: projection.thread.title,
                    items: projection.visible_turn_items,
                });
                self.history_sequence = Some(sequence);
                self.history_error = None;
                self.history_live = true;
                let mut notes: Vec<_> = self
                    .transcript
                    .as_ref()
                    .into_iter()
                    .flat_map(|history| history.items.iter())
                    .collect();
                notes.sort_by_key(|entry| entry.position);
                let notes: Vec<_> = notes
                    .into_iter()
                    .filter_map(|entry| {
                        let id = entry
                            .item
                            .id
                            .clone()
                            .or_else(|| entry.source_item_id.clone())?;
                        let summary = item_activity_summary(&entry.item)?;
                        Some((id, summary))
                    })
                    .collect();
                self.activity.clear();
                for (id, summary) in notes {
                    self.record_activity(id, summary);
                }
                if self.runs.values().any(|run| run.active())
                    || (self.runs.is_empty()
                        && self.transcript.as_ref().is_some_and(|history| {
                            history.items.iter().any(|entry| entry.item.streaming)
                        }))
                {
                    self.start_work_timer();
                } else if !self.runs.is_empty() {
                    self.stop_work_timer();
                }
            }
            LiveUpdate::Event { sequence, event } => {
                let Some(previous) = self.history_sequence else {
                    return;
                };
                if sequence <= previous {
                    return;
                }
                self.history_sequence = Some(sequence);
                match event.kind.as_str() {
                    "checkpoint.captured" => {
                        if let Ok(checkpoint) = serde_json::from_value(event.payload) {
                            self.apply_checkpoint(checkpoint);
                        }
                    }
                    "turn-item.updated" => {
                        if let Ok(item) =
                            serde_json::from_value::<crate::history::Item>(event.payload)
                        {
                            self.record_item_activity(&item);
                            if item.kind == "assistant_message" && item.streaming {
                                self.start_work_timer();
                            }
                            if let Some(history) = &mut self.transcript {
                                history.upsert(item);
                            }
                        }
                    }
                    "thread.created"
                    | "thread.metadata-updated"
                    | "thread.model-selection-updated"
                    | "thread.provider-switched" => {
                        if let Some(selection) = event.payload.get("modelSelection")
                            && let Ok(selection) = serde_json::from_value(selection.clone())
                        {
                            self.set_thread_model(thread_id, selection);
                        }
                        if let Some(title) =
                            event.payload.get("title").and_then(|value| value.as_str())
                            && let Some(history) = &mut self.transcript
                        {
                            history.title = title.into();
                        }
                    }
                    "run.created" | "run.updated" => {
                        if let Ok(run) =
                            serde_json::from_value::<crate::history::Run>(event.payload.clone())
                        {
                            self.runs.insert(run.id.clone(), run);
                        }
                        if let Some(status) =
                            event.payload.get("status").and_then(|value| value.as_str())
                        {
                            let label = match status {
                                "preparing" => "Getting ready",
                                "queued" => "Queued",
                                "starting" => "Starting",
                                "running" => "Working",
                                "waiting" => "Waiting",
                                "completed" => "Finished",
                                "interrupted" => "Interrupted",
                                "failed" => "Run failed",
                                "cancelled" => "Cancelled",
                                "rolled_back" => "Rolled back",
                                _ => "Agent update",
                            };
                            let id = event
                                .payload
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or("current-run");
                            self.activity.retain(|entry| entry.id != id);
                            if matches!(
                                status,
                                "preparing" | "queued" | "starting" | "running" | "waiting"
                            ) {
                                self.record_activity(id.into(), format!("Agent · {label}"));
                            }
                            if self.runs.values().any(|run| run.active())
                                || matches!(
                                    status,
                                    "preparing" | "queued" | "starting" | "running" | "waiting"
                                )
                            {
                                self.start_work_timer();
                            } else {
                                self.stop_work_timer();
                            }
                        }
                    }
                    _ => (),
                }
            }
            LiveUpdate::Synchronized => {
                self.history_live = true;
                self.history_error = None;
            }
            LiveUpdate::Unknown => (),
        }
    }

    pub fn apply_history(
        &mut self,
        thread_id: &str,
        result: Result<crate::history::Transcript, String>,
    ) {
        if self.history_thread.as_deref() != Some(thread_id) {
            return;
        }
        self.chat_revision = self.chat_revision.wrapping_add(1);
        match result {
            Ok(transcript) => {
                self.transcript = Some(transcript);
                self.history_error = None;
            }
            Err(error) => self.history_error = Some(error),
        }
    }

    pub fn scroll_chat(&mut self, down: bool, amount: u16) {
        self.chat_scroll = if down {
            self.chat_scroll
                .saturating_add(amount)
                .min(self.chat_max_scroll)
        } else {
            self.chat_scroll.saturating_sub(amount)
        };
        self.follow_chat = self.chat_scroll == self.chat_max_scroll;
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.sidebar_state
            .selected()
            .and_then(|index| self.rows.get(index))
            .map(|row| &row.selection)
    }

    pub fn selected_location(&self) -> Option<crate::preferences::Location> {
        let catalog = self.catalog.as_ref()?;
        match self.selection()? {
            Selection::Project(id) => Some(crate::preferences::Location {
                project_id: id.clone(),
                thread_id: None,
            }),
            Selection::Thread(id) => {
                catalog
                    .threads
                    .iter()
                    .find(|thread| &thread.id == id)
                    .map(|thread| crate::preferences::Location {
                        project_id: thread.project_id.clone(),
                        thread_id: Some(id.clone()),
                    })
            }
        }
    }

    pub fn update_catalog(&mut self, catalog: Catalog) {
        let previous = self.selection().cloned();
        let old_index = self.sidebar_state.selected().unwrap_or(0);
        self.rows = catalog.rows();
        let restored = if self.rows.is_empty() {
            None
        } else {
            self.restore_location.take().map(|location| {
                location
                    .thread_id
                    .and_then(|id| {
                        self.rows
                            .iter()
                            .position(|row| row.selection == Selection::Thread(id.clone()))
                    })
                    .or_else(|| {
                        self.rows.iter().position(|row| {
                            row.selection == Selection::Project(location.project_id.clone())
                        })
                    })
                    .unwrap_or(0)
            })
        };
        let index = restored
            .or_else(|| {
                previous.and_then(|key| self.rows.iter().position(|row| row.selection == key))
            })
            .or_else(|| {
                (!self.rows.is_empty()).then_some(old_index.min(self.rows.len().saturating_sub(1)))
            });
        self.sidebar_state.select(index);
        self.catalog = Some(catalog);
        self.catalog_error = None;
    }

    pub fn clear_catalog(&mut self) {
        if let Some(location) = self.selected_location() {
            self.restore_location = Some(location);
        }
        self.models.open = false;
        self.models.choices.clear();
        self.usage = crate::usage::UsageState::default();
        self.live_model = None;
        self.catalog = None;
        self.rows.clear();
        self.sidebar_state = ListState::default();
        self.catalog_error = None;
    }

    pub fn move_selection(&mut self, down: bool) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.sidebar_state.selected().unwrap_or(0);
        let next = if down {
            (current + 1).min(self.rows.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        self.sidebar_state.select(Some(next));
    }

    pub fn selection_details(&self) -> String {
        let Some(catalog) = &self.catalog else {
            return "Select a project or thread after the sidebar loads.".into();
        };
        match self.selection() {
            Some(Selection::Project(id)) => catalog
                .projects
                .iter()
                .find(|project| &project.id == id)
                .map(|project| {
                    format!(
                        "Project: {}\nWorkspace: {}\nThreads: {}",
                        project.title,
                        project.workspace_root,
                        catalog
                            .threads
                            .iter()
                            .filter(|thread| thread.project_id == project.id)
                            .count()
                    )
                })
                .unwrap_or_default(),
            Some(Selection::Thread(id)) => catalog
                .threads
                .iter()
                .find(|thread| &thread.id == id)
                .map(|thread| {
                    format!(
                        "Thread: {}\nProject: {}\nStatus: {}",
                        thread.title,
                        catalog
                            .projects
                            .iter()
                            .find(|project| project.id == thread.project_id)
                            .map(|project| project.title.as_str())
                            .unwrap_or("Unknown"),
                        thread.status
                    )
                })
                .unwrap_or_default(),
            None => "No project or thread selected.".into(),
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Projects => Focus::Chat,
            Focus::Chat => Focus::Composer,
            Focus::Composer => Focus::Model,
            Focus::Model => Focus::Workspace,
            Focus::Workspace => Focus::WorkspaceTabs,
            Focus::WorkspaceTabs => Focus::Projects,
        };
    }

    pub fn previous_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Projects => Focus::WorkspaceTabs,
            Focus::Chat => Focus::Projects,
            Focus::Composer => Focus::Chat,
            Focus::Model => Focus::Composer,
            Focus::Workspace => Focus::Model,
            Focus::WorkspaceTabs => Focus::Workspace,
        };
    }
}

fn item_activity_summary(item: &crate::history::Item) -> Option<String> {
    if !matches!(
        item.status.as_deref(),
        Some("pending" | "running" | "waiting")
    ) {
        return None;
    }
    let label = match item.kind.as_str() {
        "command_execution" => "Running a command",
        "file_change" => "Updating files",
        "web_search" => "Searching the web",
        "file_search" => "Searching files",
        _ => return None,
    };
    let label = item
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(label);
    let status = match item.status.as_deref() {
        Some("pending") => "queued",
        Some("running") => "in progress",
        Some("waiting") => "waiting",
        Some("completed") => "done",
        Some("failed") => "failed",
        Some("cancelled") => "cancelled",
        Some("interrupted") => "interrupted",
        _ => "updated",
    };
    Some(format!("{label} · {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_thread_selection_across_refresh_and_handles_removed_items() {
        let mut app = App::default();
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        assert!(matches!(app.selection(), Some(Selection::Thread(id)) if id == "t1"));
        let mut refreshed = crate::catalog::tests::snapshot();
        refreshed.projects.reverse();
        refreshed.normalize();
        app.update_catalog(refreshed);
        assert!(matches!(app.selection(), Some(Selection::Thread(id)) if id == "t1"));
        let mut empty = crate::catalog::tests::snapshot();
        empty.projects.clear();
        empty.threads.clear();
        empty.archived_threads.clear();
        app.update_catalog(empty);
        app.move_selection(true);
        assert!(app.selection().is_none());
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    #[test]
    fn ignores_late_results_and_keeps_read_position_during_refresh() {
        let mut app = App {
            authenticated: true,
            ..App::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        assert!(app.sync_history_selection());
        app.apply_history("t1", Ok(crate::history::tests::transcript("t1")));
        assert!(app.transcript.is_some());
        app.chat_max_scroll = 20;
        app.chat_scroll = 20;
        app.scroll_chat(false, 5);
        app.apply_history("t1", Ok(crate::history::tests::transcript("t1")));
        assert_eq!(app.chat_scroll, 15);
        assert!(!app.follow_chat);
        app.apply_history("t1", Err("temporary failure".into()));
        assert!(app.transcript.is_some());
        assert!(app.history_error.is_some());
        app.move_selection(true);
        app.sync_history_selection();
        app.apply_history("t1", Ok(crate::history::tests::transcript("t1")));
        assert!(app.transcript.is_none());
        assert_eq!(app.history_thread.as_deref(), Some("t2"));
        app.authenticated = false;
        app.sync_history_selection();
        assert!(app.history_thread.is_none());
    }
}

#[cfg(test)]
mod sending_tests {
    use super::*;

    fn ready() -> App {
        let mut app = App {
            authenticated: true,
            ..App::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.draft = "Hello\n世界".into();
        app
    }

    #[test]
    fn failed_sends_keep_draft_and_reuse_command_id_on_retry() {
        let mut app = ready();
        let request = app.prepare_send().unwrap();
        assert!(app.prepare_send().is_none());
        app.finish_send(
            request.clone(),
            Err(crate::message::SendFailure::uncertain("connection lost")),
        );
        assert_eq!(app.draft, request.text);
        let retry = app.prepare_send().unwrap();
        assert_eq!(retry.command_id, request.command_id);
        assert_eq!(retry.message_id, request.message_id);
        app.finish_send(retry, Ok(()));
        assert!(app.draft.is_empty());
        assert!(app.send_pending.is_none());
        assert!(app.prepare_send().is_none());
    }

    #[test]
    fn explicit_rejection_uses_a_new_command_id_when_retried() {
        let mut app = ready();
        let request = app.prepare_send().unwrap();
        app.finish_send(request.clone(), Err("rejected".into()));
        let retry = app.prepare_send().unwrap();
        assert_ne!(request.command_id, retry.command_id);
        assert_eq!(app.draft, request.text);
    }

    #[test]
    fn acknowledgement_does_not_clear_a_different_threads_draft() {
        let mut app = ready();
        let request = app.prepare_send().unwrap();
        app.move_selection(true);
        app.sync_history_selection();
        assert!(app.draft.is_empty());
        app.draft = "Draft for second thread".into();
        app.finish_send(request, Ok(()));
        assert_eq!(app.draft, "Draft for second thread");
        app.move_selection(false);
        app.sync_history_selection();
        assert!(app.draft.is_empty());
    }

    #[test]
    fn acknowledgement_keeps_text_edited_while_sending() {
        let mut app = ready();
        let request = app.prepare_send().unwrap();
        app.draft = "Next message".into();
        app.finish_send(request, Ok(()));
        assert_eq!(app.draft, "Next message");
        app.authenticated = false;
        assert!(app.prepare_send().is_none());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn snapshot_and_live_checkpoint_use_server_file_summaries() {
        let mut app = App {
            history_thread: Some("t1".into()),
            ..Default::default()
        };
        let mut initial = snapshot(5);
        initial["projection"]["checkpoints"] = json!([{
            "runId":"r1", "status":"ready", "files":[
                {"path":"src/main.rs","additions":2,"deletions":1}
            ]
        }]);
        apply(&mut app, initial);
        assert_eq!(app.run_diffs["r1"]["src/main.rs"], (2, 1));
        apply(
            &mut app,
            json!({"kind":"event","sequence":6,"event":{
                "type":"checkpoint.captured","payload":{"runId":"r1","status":"ready","files":[]}
            }}),
        );
        assert!(app.run_diffs["r1"].is_empty());
    }

    fn apply(app: &mut App, value: Value) {
        app.apply_live("t1", Ok(serde_json::from_value(value).unwrap()));
    }

    fn snapshot(sequence: u64) -> Value {
        json!({"kind":"snapshot","snapshotSequence":sequence,"projection":{"thread":{"id":"t1","title":"Conversation"},"visibleTurnItems":[
            {"position":0,"sourceItemId":"u1","item":{"id":"u1","type":"user_message","text":"Question"}}
        ]}})
    }

    fn event(sequence: u64, text: &str, streaming: bool) -> Value {
        json!({"kind":"event","sequence":sequence,"event":{"type":"turn-item.updated","payload":{
            "id":"a1","type":"assistant_message","text":text,"streaming":streaming
        }}})
    }

    #[test]
    fn updates_partial_text_in_place_rejects_duplicate_events_and_preserves_scroll() {
        let mut app = App {
            history_thread: Some("t1".into()),
            ..Default::default()
        };
        apply(&mut app, snapshot(5));
        app.chat_scroll = 3;
        app.follow_chat = false;
        apply(&mut app, event(6, "Hel", true));
        assert!(
            app.transcript
                .as_ref()
                .unwrap()
                .text()
                .contains("responding")
        );
        apply(&mut app, event(7, "Hello", false));
        apply(&mut app, event(6, "Hel", true));
        assert_eq!(app.transcript.as_ref().unwrap().items.len(), 2);
        assert!(app.transcript.as_ref().unwrap().text().contains("Hello"));
        assert!(
            !app.transcript
                .as_ref()
                .unwrap()
                .text()
                .contains("responding")
        );
        assert_eq!(app.chat_scroll, 3);
        assert!(!app.follow_chat);
        app.apply_live("t2", Err("late wrong-thread error".into()));
        assert!(app.history_error.is_none());
        app.apply_live("t1", Err("reconnecting".into()));
        assert!(!app.history_live);
        assert!(app.transcript.is_some());
        apply(&mut app, snapshot(8));
        assert_eq!(app.transcript.as_ref().unwrap().items.len(), 1);
        assert!(app.history_live);
        assert!(app.history_error.is_none());
    }

    #[test]
    fn thinking_timer_tracks_active_runs_and_stops_when_the_run_finishes() {
        let mut app = App {
            history_thread: Some("t1".into()),
            ..Default::default()
        };
        apply(&mut app, snapshot(5));
        app.apply_live(
            "t1",
            Ok(serde_json::from_value(json!({
                "kind":"event","sequence":6,
                "event":{"type":"run.updated","payload":{"status":"running"}}
            }))
            .unwrap()),
        );
        assert!(app.thinking_since.is_some());
        assert!(app.worked_duration.is_none());
        let started = app.thinking_since;
        apply(&mut app, event(7, "Progress message finished", false));
        assert_eq!(app.thinking_since, started);
        app.apply_live(
            "t1",
            Ok(serde_json::from_value(json!({
                "kind":"event","sequence":8,
                "event":{"type":"run.updated","payload":{"status":"completed"}}
            }))
            .unwrap()),
        );
        assert!(app.thinking_since.is_none());
        assert!(app.worked_duration.is_some());
        let completed_duration = app.worked_duration;
        apply(&mut app, snapshot(9));
        assert_eq!(app.worked_duration, completed_duration);
    }

    #[test]
    fn live_activity_feed_keeps_recent_progress_and_updates_existing_items() {
        let mut app = App {
            history_thread: Some("t1".into()),
            ..Default::default()
        };
        apply(&mut app, snapshot(5));
        app.apply_live(
            "t1",
            Ok(serde_json::from_value(json!({
                "kind":"event","sequence":6,
                "event":{"type":"turn-item.updated","payload":{
                    "id":"tool-1","type":"command_execution","title":"Running tests","status":"running"
                }}
            })).unwrap()),
        );
        assert_eq!(app.activity.len(), 1);
        assert!(
            app.activity[0]
                .summary
                .contains("Running tests · in progress")
        );
        app.apply_live(
            "t1",
            Ok(serde_json::from_value(json!({
                "kind":"event","sequence":7,
                "event":{"type":"turn-item.updated","payload":{
                    "id":"tool-1","type":"command_execution","title":"Running tests","status":"completed"
                }}
            })).unwrap()),
        );
        assert!(app.activity.is_empty());
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::models::{ModelSelection, choices};

    fn ready() -> App {
        let mut app = App {
            authenticated: true,
            ..Default::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        catalog.threads[0].model_selection = Some(ModelSelection {
            instance_id: "codex".into(),
            model: "model-a".into(),
            options: Some(serde_json::json!({"effort":"high"})),
        });
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.models.choices = choices(crate::models::tests::catalog()).unwrap();
        app
    }

    #[test]
    fn picker_does_not_change_model_until_confirmed_and_acknowledged() {
        let mut app = ready();
        app.open_models();
        app.move_model(true, 1);
        assert_eq!(app.current_model().unwrap().model, "model-a");
        app.models.open = false;
        assert_eq!(app.current_model().unwrap().model, "model-a");
        app.open_models();
        // Selecting the already-current model must preserve its options.
        assert!(app.prepare_model_change().is_none());
        assert!(app.current_model().unwrap().options.is_some());
        app.open_models();
        app.move_model(true, 1);
        let request = app.prepare_model_change().unwrap();
        assert_eq!(app.current_model().unwrap().model, "model-a");
        app.draft = "keep my prompt".into();
        assert!(app.prepare_send().is_none());
        app.finish_model_change(request, Ok(()));
        assert_eq!(app.current_model().unwrap().model, "model-b");
        assert_eq!(app.draft, "keep my prompt");
    }

    #[test]
    fn model_changes_from_the_gui_update_the_current_model_through_live_events() {
        let mut app = ready();
        app.history_sequence = Some(10);
        let update = serde_json::json!({"kind":"event","sequence":11,"event":{"type":"thread.model-selection-updated","payload":{"modelSelection":{"instanceId":"codex","model":"model-b"}}}});
        app.apply_live("t1", Ok(serde_json::from_value(update).unwrap()));
        assert_eq!(app.current_model().unwrap().model, "model-b");
    }

    #[test]
    fn failed_or_late_changes_do_not_overwrite_another_threads_model() {
        let mut app = ready();
        app.open_models();
        app.move_model(true, 1);
        let request = app.prepare_model_change().unwrap();
        app.finish_model_change(request, Err("Rejected".into()));
        assert_eq!(app.current_model().unwrap().model, "model-a");
        app.open_models();
        app.move_model(true, 1);
        let request = app.prepare_model_change().unwrap();
        app.move_selection(true);
        app.sync_history_selection();
        app.finish_model_change(request, Ok(()));
        assert!(app.current_model().is_none());
        app.move_selection(false);
        app.sync_history_selection();
        assert_eq!(app.current_model().unwrap().model, "model-b");
    }
}
