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

pub enum ClipboardStatus {
    Checking,
    Available,
    Unavailable(String),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Projects,
    Chat,
    Composer,
    Model,
    Access,
    SessionUsage,
    WeeklyUsage,
    Workspace,
    WorkspaceTabs,
}

pub const RUNTIME_MODES: [(&str, &str, &str); 4] = [
    (
        "approval-required",
        "Approval required",
        "Ask before running tools or editing",
    ),
    (
        "auto-accept-edits",
        "Auto-accept edits",
        "Auto-approve edits, ask for commands",
    ),
    ("auto", "Auto", "Run autonomously with safety checks"),
    ("full-access", "Full access", "No approval prompts"),
];

pub fn runtime_mode_label(mode: &str) -> &str {
    RUNTIME_MODES
        .iter()
        .find(|(id, _, _)| *id == mode)
        .map_or(mode, |(_, label, _)| *label)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApproval {
    pub request_id: String,
    pub kind: String,
    pub prompt: Option<String>,
    pub reason: Option<String>,
    pub app_name: Option<String>,
    pub options: Vec<crate::history::ApprovalOption>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitDialog {
    pub shutdown_server: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadMenuMode {
    ProjectOptions,
    Options,
    Rename,
    ConfirmDelete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadMenu {
    pub thread_id: String,
    pub original_title: String,
    pub mode: ThreadMenuMode,
    pub selected_option: usize,
    pub rename_input: String,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "clear",
        description: "Start a fresh session; earlier messages stay visible",
    },
    SlashCommand {
        name: "context",
        description: "Show the current thread's context window usage in chat",
    },
    SlashCommand {
        name: "model",
        description: "Switch response model for this thread",
    },
];

#[derive(Clone, Copy)]
pub enum ClickTarget {
    StartupPrimary,
    StartupClose,
    QuitChoice(bool),
    QuitCancel,
    ThreadMenuOption(usize),
    ThreadMenuConfirm,
    ThreadMenuCancel,
    SlashCommand(usize),
    AutoStart(bool),
    Telemetry(bool),
    GitControl(usize),
    GitGraph,
    GitModeOption(usize),
    GitBranchOption(usize),
    GitMessage,
    GitFiles,
    EditorEntry(usize),
    RemoveAttachment(usize),
    SidebarRow(usize),
    DiffBody,
    DiffFile(usize),
    Pane(Focus),
    Workspace(usize),
    TypingStyle(crate::preferences::TypingStyle),
    LayoutPreset(crate::preferences::LayoutPreset),
    ServerUpdateButton,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ServerVersionStatus {
    #[default]
    Unknown,
    Checking,
    UpToDate {
        latest: String,
    },
    UpdateAvailable {
        latest: String,
        database_migration: Option<u64>,
    },
    Error(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ServerUpdateStatus {
    #[default]
    Idle,
    Downloading {
        target_version: String,
        tick: u8,
    },
    Installing {
        target_version: String,
    },
    Completed {
        target_version: String,
    },
    Failed(String),
}

#[derive(Default)]
pub struct ThreadDiffs {
    pub view: crate::diff::DiffView,
    pub counts: HashMap<String, crate::history::FileCounts>,
    pub requested: std::collections::HashSet<String>,
}

pub struct App {
    pub thread_diffs: HashMap<String, ThreadDiffs>,
    pub startup: crate::startup::State,
    pub auto_start_t3: bool,
    pub auto_start_highlight: bool,
    pub telemetry_enabled: bool,
    pub telemetry_highlight: bool,
    pub settings_row: usize,
    pub editor: crate::editor::EditorState,
    pub git: crate::git::GitState,
    pub chat_revision: u64,
    pub chat_cache: Option<(u64, Option<u64>, ratatui::text::Text<'static>)>,
    pub chat_viewport: Option<crate::chat::Viewport>,
    pub diff_view: crate::diff::DiffView,
    pub run_diffs: HashMap<String, crate::history::FileCounts>,
    pub diff_requested: std::collections::HashSet<String>,
    pub click_targets: Vec<(ratatui::layout::Rect, ClickTarget)>,
    pub workspace_tab: usize,
    pub workspace_highlight: usize,
    pub workspace_visible: bool,
    pub workspace_width: u16,
    pub layout_preset: crate::preferences::LayoutPreset,
    pub layout_highlight: crate::preferences::LayoutPreset,
    pub diff_wrap_highlight: bool,
    pub style_highlight: crate::preferences::TypingStyle,
    pub settings_open: bool,
    pub clipboard_status: ClipboardStatus,
    pub clipboard_check_requested: bool,
    pub clipboard_check_pending: bool,
    pub terminal_overlay_open: bool,
    pub local_terminal: crate::local_terminal::LocalTerminal,
    pub typing_style: crate::preferences::TypingStyle,
    pub focus: Focus,
    pub models: crate::models::ModelPicker,
    pub usage: crate::usage::UsageState,
    pub conversation_messages: HashMap<String, crate::history::ConversationMessage>,
    pub pointer: Option<ratatui::layout::Position>,
    pub live_model: Option<crate::models::ModelSelection>,
    pub live_runtime_mode: Option<String>,
    pub context_usage: Option<crate::history::ContextUsage>,
    pub context_report_thread: Option<String>,
    pub access_open: bool,
    pub access_highlight: usize,
    pub access_pending: Option<crate::t3::RuntimeModeChange>,
    pub access_requested: Option<crate::t3::RuntimeModeChange>,
    pub access_error: Option<String>,
    pub runtime_requests: HashMap<String, crate::history::RuntimeRequest>,
    pub approval_highlight: usize,
    pub approval_dismissed_id: Option<String>,
    pub approval_pending: Option<crate::t3::ApprovalResponse>,
    pub approval_requested: Option<crate::t3::ApprovalResponse>,
    pub approval_error: Option<String>,
    pub draft: String,
    pub draft_cursor: usize,
    pub(crate) draft_history: HashMap<Option<String>, crate::composer::DraftHistory>,
    pub draft_selection_anchor: Option<usize>,
    pub screen_selection: Option<((u16, u16), (u16, u16))>,
    pub screen_selection_area: Option<ratatui::layout::Rect>,
    pub chat_text_area: ratatui::layout::Rect,
    pub chat_selection_scroll: u16,
    pub screen_selecting: bool,
    pub screen_buffer: Option<ratatui::buffer::Buffer>,
    pub copy_requested: Option<String>,
    pub context_menu: Option<(u16, u16)>,
    pub context_menu_area: ratatui::layout::Rect,
    pub context_paste_requested: bool,
    pub composer_text_area: ratatui::layout::Rect,
    pub last_escape: Option<Instant>,
    pub editing: bool,
    pub quit: bool,
    pub quit_dialog: Option<QuitDialog>,
    pub local_server_pid: Option<u32>,
    pub shutdown_server_on_quit: bool,
    pub check_other_instances: Option<fn() -> bool>,
    pub thread_menu: Option<ThreadMenu>,
    pub sidebar_menu_anchor: (u16, u16),
    pub thread_action_requested: Option<crate::t3::ThreadAction>,
    pub fresh_context_threads: std::collections::HashSet<String>,
    pub known_running_threads: std::collections::HashSet<String>,
    pub finished_threads: std::collections::HashSet<String>,
    pub slash_highlight: usize,
    pub slash_dismissed: bool,
    pub connection: String,
    /// Machine label reported by the connected T3 environment descriptor.
    pub environment_label: Option<String>,
    pub server_version: Option<String>,
    pub server_connected: bool,
    pub server_version_status: ServerVersionStatus,
    pub server_update_status: ServerUpdateStatus,
    pub server_version_check_requested: bool,
    pub server_update_requested: bool,
    pub tui3_update: crate::update::Status,
    pub dictation: crate::dictation::State,
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
    pub steer_requested: bool,
    pub paste_requested: bool,
    pub paste_pending: bool,
    pub paste_generation: u64,
    pub images: HashMap<String, Vec<serde_json::Value>>,
    pub send_pending: Option<SendRequest>,
    pub send_notice: Option<String>,
    pub send_status: HashMap<String, String>,
    pub last_attempt: HashMap<String, SendRequest>,
    pub previous_messages: HashMap<String, String>,
    pub draft_thread: Option<String>,
    pub drafts: HashMap<Option<String>, String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            thread_diffs: HashMap::new(),
            startup: Default::default(),
            auto_start_t3: true,
            auto_start_highlight: true,
            telemetry_enabled: false,
            telemetry_highlight: false,
            settings_row: 0,
            environment_label: None,
            server_version: None,
            server_connected: false,
            server_version_status: ServerVersionStatus::Unknown,
            server_update_status: ServerUpdateStatus::Idle,
            server_version_check_requested: false,
            server_update_requested: false,
            tui3_update: crate::update::Status::default(),
            dictation: Default::default(),
            editor: Default::default(),
            git: Default::default(),
            chat_revision: 0,
            chat_cache: None,
            chat_viewport: None,
            diff_view: Default::default(),
            run_diffs: HashMap::new(),
            diff_requested: Default::default(),
            click_targets: Vec::new(),
            workspace_tab: 0,
            workspace_highlight: 0,
            workspace_visible: true,
            workspace_width: crate::preferences::LayoutPreset::default().workspace_width(),
            layout_preset: crate::preferences::LayoutPreset::default(),
            layout_highlight: crate::preferences::LayoutPreset::default(),
            diff_wrap_highlight: true,
            style_highlight: crate::preferences::TypingStyle::default(),
            settings_open: false,
            clipboard_status: ClipboardStatus::Checking,
            clipboard_check_requested: true,
            clipboard_check_pending: false,
            terminal_overlay_open: false,
            local_terminal: Default::default(),
            typing_style: crate::preferences::TypingStyle::default(),
            focus: Focus::default(),
            models: crate::models::ModelPicker::default(),
            usage: crate::usage::UsageState::default(),
            conversation_messages: HashMap::new(),
            pointer: None,
            live_model: None,
            live_runtime_mode: None,
            context_usage: None,
            context_report_thread: None,
            access_open: false,
            access_highlight: 0,
            access_pending: None,
            access_requested: None,
            access_error: None,
            runtime_requests: HashMap::new(),
            approval_highlight: 0,
            approval_dismissed_id: None,
            approval_pending: None,
            approval_requested: None,
            approval_error: None,
            draft: String::new(),
            draft_cursor: 0,
            draft_history: HashMap::new(),
            draft_selection_anchor: None,
            screen_selection: None,
            screen_selection_area: None,
            chat_text_area: Default::default(),
            chat_selection_scroll: 0,
            screen_selecting: false,
            screen_buffer: None,
            copy_requested: None,
            context_menu: None,
            context_menu_area: Default::default(),
            context_paste_requested: false,
            composer_text_area: Default::default(),
            last_escape: None,
            editing: false,
            quit: false,
            quit_dialog: None,
            local_server_pid: None,
            shutdown_server_on_quit: false,
            check_other_instances: None,
            thread_menu: None,
            sidebar_menu_anchor: (0, 0),
            thread_action_requested: None,
            fresh_context_threads: std::collections::HashSet::new(),
            known_running_threads: std::collections::HashSet::new(),
            finished_threads: std::collections::HashSet::new(),
            slash_highlight: 0,
            slash_dismissed: false,
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
            steer_requested: false,
            paste_requested: false,
            paste_pending: false,
            paste_generation: 0,
            images: HashMap::new(),
            send_pending: None,
            send_notice: None,
            send_status: HashMap::new(),
            last_attempt: HashMap::new(),
            previous_messages: HashMap::new(),
            draft_thread: None,
            drafts: HashMap::new(),
        }
    }
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> Option<(u64, u64, u64, String)> {
        let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, pre.to_owned()),
            None => (s, String::new()),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor, patch, pre))
    };
    match (parse(current), parse(latest)) {
        (Some((c_maj, c_min, c_pat, c_pre)), Some((l_maj, l_min, l_pat, l_pre))) => {
            if (l_maj, l_min, l_pat) != (c_maj, c_min, c_pat) {
                (l_maj, l_min, l_pat) > (c_maj, c_min, c_pat)
            } else {
                // Stable release (empty pre) is newer than a prerelease of the same core version.
                match (c_pre.is_empty(), l_pre.is_empty()) {
                    (false, true) => true,
                    (true, false) => false,
                    _ => l_pre != c_pre && l_pre > c_pre,
                }
            }
        }
        _ => current.trim() != latest.trim(),
    }
}

impl App {
    pub fn request_server_version_check(&mut self) {
        if self.authenticated
            && !matches!(
                self.server_update_status,
                ServerUpdateStatus::Downloading { .. } | ServerUpdateStatus::Installing { .. }
            )
        {
            self.server_version_status = ServerVersionStatus::Checking;
            self.server_version_check_requested = true;
        }
    }

    pub fn release_spec(&self) -> &'static str {
        if self
            .server_version
            .as_deref()
            .is_some_and(|v| v.contains("nightly"))
        {
            "nightly"
        } else {
            "latest"
        }
    }

    pub fn trigger_server_update_action(&mut self) {
        if matches!(
            self.server_update_status,
            ServerUpdateStatus::Downloading { .. } | ServerUpdateStatus::Installing { .. }
        ) {
            return;
        }
        if !self.authenticated {
            self.server_version_status =
                ServerVersionStatus::Error("Connect and sign in to T3 first.".into());
            return;
        }
        match &self.server_version_status {
            ServerVersionStatus::UpdateAvailable { latest, .. } => {
                self.server_update_status = ServerUpdateStatus::Downloading {
                    target_version: latest.clone(),
                    tick: 12,
                };
                self.server_update_requested = true;
            }
            ServerVersionStatus::Checking => (),
            _ => {
                self.server_update_status = ServerUpdateStatus::Idle;
                self.server_version_status = ServerVersionStatus::Checking;
                self.server_version_check_requested = true;
            }
        }
    }

    pub fn apply_server_version_check(
        &mut self,
        result: Result<Option<crate::t3::NpmRelease>, String>,
    ) {
        match result {
            Ok(Some(release)) => {
                let is_newer = self
                    .server_version
                    .as_deref()
                    .is_some_and(|current| is_newer_version(current, &release.version));
                if is_newer {
                    self.server_version_status = ServerVersionStatus::UpdateAvailable {
                        latest: release.version,
                        database_migration: release.database_migration,
                    };
                } else {
                    self.server_version_status = ServerVersionStatus::UpToDate {
                        latest: release.version,
                    };
                }
            }
            Ok(None) => {
                self.server_version_status =
                    ServerVersionStatus::Error("No T3 release found on npm.".into());
            }
            Err(error) => {
                self.server_version_status = ServerVersionStatus::Error(error);
            }
        }
    }

    pub fn tick_server_update(&mut self) {
        if let ServerUpdateStatus::Downloading { tick, .. } = &mut self.server_update_status
            && *tick < 72
        {
            *tick = (*tick).saturating_add(2).min(72);
        }
    }

    pub fn finish_diff(
        &mut self,
        thread_id: String,
        run_id: String,
        result: Result<String, String>,
    ) {
        if self.history_thread.as_ref() == Some(&thread_id) {
            self.chat_revision = self.chat_revision.wrapping_add(1);
            Self::store_diff(&mut self.diff_view, &mut self.run_diffs, run_id, result);
        } else {
            let cached = self.thread_diffs.entry(thread_id).or_default();
            Self::store_diff(&mut cached.view, &mut cached.counts, run_id, result);
        }
    }

    fn store_diff(
        view: &mut crate::diff::DiffView,
        counts: &mut HashMap<String, crate::history::FileCounts>,
        run_id: String,
        result: Result<String, String>,
    ) {
        match result {
            Ok(patch) => {
                counts
                    .entry(run_id.clone())
                    .or_insert_with(|| crate::history::diff_counts(&patch));
                view.errors.remove(&run_id);
                view.patches.insert(run_id, patch.into());
            }
            Err(error) => {
                view.errors.insert(run_id, error);
            }
        }
    }
    pub fn attach_clipboard_image(&mut self, thread: String, image: serde_json::Value) {
        let images = self.images.entry(thread).or_default();
        let total = images
            .iter()
            .chain(std::iter::once(&image))
            .map(|image| image["sizeBytes"].as_u64().unwrap_or(0))
            .sum::<u64>();
        if images.len() >= 100 || total > 80 * 1024 * 1024 {
            self.send_notice =
                Some("T3 allows at most 100 images totaling 80 MiB per message.".into());
            return;
        }
        images.push(image);
        self.send_notice =
            Some("Clipboard image attached. Enter sends it; Ctrl+Backspace removes images.".into());
    }
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

    pub fn selected_draft(&self) -> Option<String> {
        let anchor = self.draft_selection_anchor?;
        let (start, end) = (anchor.min(self.draft_cursor), anchor.max(self.draft_cursor));
        (start < end).then(|| self.draft.chars().skip(start).take(end - start).collect())
    }

    fn delete_draft_selection(&mut self) -> bool {
        let Some(anchor) = self.draft_selection_anchor.take() else {
            return false;
        };
        let (start, end) = (anchor.min(self.draft_cursor), anchor.max(self.draft_cursor));
        if start == end {
            return false;
        }
        let start_byte = self
            .draft
            .char_indices()
            .nth(start)
            .map_or(self.draft.len(), |(i, _)| i);
        let end_byte = self
            .draft
            .char_indices()
            .nth(end)
            .map_or(self.draft.len(), |(i, _)| i);
        self.draft.drain(start_byte..end_byte);
        self.draft_cursor = start;
        true
    }

    pub fn selected_screen(&self) -> Option<String> {
        let ((ay, ax), (by, bx)) = self.screen_selection?;
        let buffer = self.screen_buffer.as_ref()?;
        let ((start_y, start_x), (end_y, end_x)) = if (ay, ax) <= (by, bx) {
            ((ay, ax), (by, bx))
        } else {
            ((by, bx), (ay, ax))
        };
        if (start_y, start_x) == (end_y, end_x) {
            return None;
        }
        let area = self.screen_selection_area.unwrap_or(buffer.area);
        let chat = self.screen_selection_area == Some(self.chat_text_area);
        let mut lines = Vec::new();
        for y in start_y..=end_y {
            if y < area.y || y >= area.bottom() {
                continue;
            }
            let left = if y == start_y { start_x } else { area.x };
            let right = if y == end_y { end_x } else { area.right() };
            let mut line = String::new();
            for x in left.max(area.x)..right.min(area.right()) {
                if let Some(cell) = buffer.cell((x, y)) {
                    line.push_str(cell.symbol());
                }
            }
            let line = line.trim_end();
            let line = if chat {
                line.strip_prefix("│ ").unwrap_or(line)
            } else {
                line
            };
            let line = line.to_owned();
            lines.push((y, line));
        }
        let mut result = String::new();
        for (index, (y, line)) in lines.into_iter().enumerate() {
            if index > 0 {
                let previous_y = y - 1;
                let wrapped = chat
                    && self.chat_viewport.as_ref().is_some_and(|viewport| {
                        viewport.same_source_line(
                            previous_y - area.y + self.chat_selection_scroll,
                            y - area.y + self.chat_selection_scroll,
                        )
                    });
                if !wrapped {
                    result.push('\n');
                }
            }
            result.push_str(&line);
        }
        Some(result)
    }

    fn draft_snapshot(&self) -> crate::composer::DraftSnapshot {
        crate::composer::DraftSnapshot {
            text: self.draft.clone(),
            cursor: self.draft_cursor,
            anchor: self.draft_selection_anchor,
        }
    }

    fn edit_draft<R>(&mut self, edit: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.draft_snapshot();
        let result = edit(self);
        if before.text != self.draft {
            self.draft_history
                .entry(self.draft_thread.clone())
                .or_default()
                .record(before);
        }
        result
    }

    pub fn undo_draft(&mut self, redo: bool) {
        let current = self.draft_snapshot();
        let history = self
            .draft_history
            .entry(self.draft_thread.clone())
            .or_default();
        let snapshot = if redo {
            history.redo.pop()
        } else {
            history.undo.pop()
        };
        if let Some(snapshot) = snapshot {
            if redo {
                history.undo.push(current);
            } else {
                history.redo.push(current);
            }
            self.draft = snapshot.text;
            self.draft_cursor = snapshot.cursor.min(self.draft.chars().count());
            self.draft_selection_anchor = snapshot
                .anchor
                .map(|anchor| anchor.min(self.draft.chars().count()));
            self.screen_selection = None;
            self.slash_dismissed = true;
            self.slash_highlight = 0;
        }
    }

    pub fn clear_draft(&mut self) {
        self.edit_draft(|app| {
            app.draft.clear();
            app.draft_cursor = 0;
            app.draft_selection_anchor = None;
        });
    }

    pub fn insert_draft(&mut self, text: &str) {
        self.edit_draft(|app| app.insert_draft_inner(text))
    }

    fn insert_draft_inner(&mut self, text: &str) {
        self.screen_selection = None;
        self.delete_draft_selection();
        let byte = self.draft_byte_cursor();
        let text = normalize_draft_text(text);
        self.draft.insert_str(byte, &text);
        self.draft_cursor += text.chars().count();
    }

    /// Turn pending dictation key requests into a backend command.
    pub fn take_dictation_command(&mut self) -> Option<crate::dictation::Command> {
        use crate::dictation::{Command, Phase};
        if std::mem::take(&mut self.dictation.cancel_requested) {
            self.dictation.toggle_requested = false;
            if self.dictation.listening() {
                self.dictation.phase = Phase::Idle;
                self.dictation.started = None;
                return Some(Command::Cancel);
            }
            return None;
        }
        if !std::mem::take(&mut self.dictation.toggle_requested) {
            return None;
        }
        match self.dictation.phase {
            Phase::Idle => {
                let Some(thread) = self.draft_thread.clone() else {
                    self.send_notice = Some("Select a thread before dictating.".into());
                    return None;
                };
                self.dictation.phase = Phase::Starting;
                self.dictation.thread = Some(thread);
                self.dictation.level = 0.0;
                self.send_notice = None;
                Some(Command::Start)
            }
            Phase::Starting | Phase::Recording => Some(Command::Stop),
            Phase::Transcribing => {
                self.send_notice = Some("Still transcribing the last dictation…".into());
                None
            }
        }
    }

    /// Stop before T3's five-minute audio limit.
    pub fn check_dictation_limit(&mut self) {
        if self.dictation.phase == crate::dictation::Phase::Recording
            && self.dictation.started.is_some_and(|started| {
                started.elapsed().as_secs() >= crate::dictation::AUTO_STOP_SECONDS
            })
            && !self.dictation.toggle_requested
        {
            self.dictation.toggle_requested = true;
            self.dictation.started = None;
        }
    }

    pub fn apply_dictation(&mut self, event: crate::dictation::Event) {
        use crate::dictation::{Event, Phase};
        match event {
            Event::Recording { .. } if self.dictation.phase == Phase::Starting => {
                self.dictation.phase = Phase::Recording;
                self.dictation.started = Some(Instant::now());
            }
            Event::Recording { .. } => (),
            Event::Level(level) => self.dictation.level = level,
            Event::Transcribing => self.dictation.phase = Phase::Transcribing,
            Event::Cancelled => {
                self.dictation = Default::default();
                self.send_notice = Some("Dictation cancelled.".into());
            }
            Event::Finished(result) => {
                let thread = self.dictation.thread.take();
                let send_after = self.dictation.send_on_finish;
                self.dictation = Default::default();
                match (result, thread) {
                    (Ok(text), Some(thread)) => {
                        let current = self.draft_thread.as_deref() == Some(thread.as_str());
                        let prefix = if current {
                            let byte = self.draft_byte_cursor();
                            self.draft[..byte].to_owned()
                        } else {
                            self.drafts
                                .get(&Some(thread.clone()))
                                .cloned()
                                .unwrap_or_default()
                        };
                        let addition = crate::dictation::join_text(&prefix, &text);
                        self.send_notice = Some(if addition.is_empty() {
                            "No speech was recognized.".into()
                        } else if self.insert_into_thread_draft(&thread, &addition) {
                            "Dictation added to Message.".into()
                        } else {
                            "Dictation added to the original thread's draft.".into()
                        });
                        if send_after && current && !self.draft.trim().is_empty() {
                            self.send_requested = true;
                            self.editing = false;
                        }
                    }
                    (Ok(_), None) => (),
                    (Err(error), _) => self.send_notice = Some(error),
                }
            }
        }
    }

    pub fn insert_into_thread_draft(&mut self, thread: &str, text: &str) -> bool {
        if self.draft_thread.as_deref() == Some(thread) {
            self.insert_draft(text);
            true
        } else {
            let draft = self
                .drafts
                .get(&Some(thread.to_owned()))
                .cloned()
                .unwrap_or_default();
            if !text.is_empty() {
                self.draft_history
                    .entry(Some(thread.to_owned()))
                    .or_default()
                    .record(crate::composer::DraftSnapshot {
                        cursor: draft.chars().count(),
                        text: draft,
                        anchor: None,
                    });
            }
            self.drafts
                .entry(Some(thread.to_owned()))
                .or_default()
                .push_str(&normalize_draft_text(text));
            false
        }
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
        self.edit_draft(|app| app.backspace_draft_inner())
    }

    fn backspace_draft_inner(&mut self) {
        self.screen_selection = None;
        if self.delete_draft_selection() {
            return;
        }
        use unicode_segmentation::UnicodeSegmentation;
        let end = self.draft_byte_cursor();
        if let Some((start, _)) = self.draft[..end].grapheme_indices(true).next_back() {
            let removed = self.draft[start..end].chars().count();
            self.draft.drain(start..end);
            self.draft_cursor -= removed;
        }
    }

    pub fn backspace_draft_word(&mut self) {
        self.edit_draft(|app| app.backspace_draft_word_inner())
    }

    fn backspace_draft_word_inner(&mut self) {
        self.screen_selection = None;
        if self.delete_draft_selection() {
            return;
        }
        use unicode_segmentation::UnicodeSegmentation;
        let end = self.draft_byte_cursor();
        if end == 0 {
            return;
        }
        let start = self.draft[..end]
            .unicode_word_indices()
            .rev()
            .find(|(start, _)| *start < end)
            .map_or(0, |(start, _)| start);
        let removed = self.draft[start..end].chars().count();
        self.draft.drain(start..end);
        self.draft_cursor -= removed;
    }

    pub fn select_all_draft(&mut self) {
        self.screen_selection = None;
        if self.draft.is_empty() {
            self.draft_selection_anchor = None;
            self.draft_cursor = 0;
        } else {
            self.draft_selection_anchor = Some(0);
            self.draft_cursor = self.draft.chars().count();
        }
    }

    pub fn is_fresh_context(&self) -> bool {
        self.selected_thread_id()
            .is_some_and(|id| self.fresh_context_threads.contains(id))
    }

    pub fn slash_query(&self) -> Option<(usize, usize, &str)> {
        if !self.editing || self.focus != Focus::Composer || self.slash_dismissed {
            return None;
        }
        let byte = self
            .draft
            .char_indices()
            .nth(self.draft_cursor)
            .map_or(self.draft.len(), |(index, _)| index);
        let prefix = &self.draft[..byte];
        let token_start = prefix.rfind(char::is_whitespace).map_or(0, |index| {
            index + prefix[index..].chars().next().map_or(1, char::len_utf8)
        });
        let token = &prefix[token_start..];
        let query = token.strip_prefix('/')?;
        Some((token_start, byte, query))
    }

    pub fn matching_slash_commands(&self) -> Vec<&'static SlashCommand> {
        let Some((_, _, query)) = self.slash_query() else {
            return Vec::new();
        };
        let query = query.to_ascii_lowercase();
        let mut prefix_matches = Vec::new();
        let mut other_matches = Vec::new();
        for command in SLASH_COMMANDS {
            if query.is_empty() || command.name.starts_with(&query) {
                prefix_matches.push(command);
            } else if command.name.contains(&query) {
                other_matches.push(command);
            }
        }
        prefix_matches.extend(other_matches);
        prefix_matches
    }

    pub fn apply_slash_command(&mut self, index: usize) -> bool {
        self.edit_draft(|app| app.apply_slash_command_inner(index))
    }

    fn apply_slash_command_inner(&mut self, index: usize) -> bool {
        let matches = self.matching_slash_commands();
        let Some((token_start, cursor_byte, _)) = self.slash_query() else {
            return false;
        };
        let Some(command) = matches.get(index).copied() else {
            return false;
        };
        self.draft.drain(token_start..cursor_byte);
        if token_start == 0 && self.draft.starts_with(' ') {
            self.draft.remove(0);
        }
        self.draft_cursor = self.draft[..token_start.min(self.draft.len())]
            .chars()
            .count();
        self.draft_selection_anchor = None;
        self.slash_highlight = 0;
        self.slash_dismissed = false;
        match command.name {
            "clear" => {
                if let Some(thread_id) = self.selected_thread_id().map(str::to_owned) {
                    self.fresh_context_threads.insert(thread_id);
                    self.send_notice = Some(
                        "Fresh context enabled: next message starts a new agent session.".into(),
                    );
                } else {
                    self.send_notice = Some("Select a thread before using /clear.".into());
                }
            }
            "context" => {
                if let Some(thread_id) = self.selected_thread_id().map(str::to_owned) {
                    self.context_report_thread = Some(thread_id);
                    self.chat_revision = self.chat_revision.wrapping_add(1);
                    self.follow_chat = true;
                    self.editing = false;
                    self.send_notice = None;
                } else {
                    self.send_notice = Some("Select a thread before using /context.".into());
                }
            }
            "model" => {
                self.editing = false;
                self.open_models();
            }
            _ => (),
        }
        true
    }

    pub fn delete_draft(&mut self) {
        self.edit_draft(|app| app.delete_draft_inner())
    }

    fn delete_draft_inner(&mut self) {
        self.screen_selection = None;
        if self.delete_draft_selection() {
            return;
        }
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

    pub fn commit_generator_target(&self) -> crate::commit_message::Target {
        let Some(selection) = self.current_model() else {
            return crate::commit_message::Target::default();
        };
        let choice = self.models.choices.iter().find(|choice| {
            choice.selection.instance_id == selection.instance_id
                && choice.selection.model == selection.model
        });
        let driver = choice.map(|choice| choice.driver.as_str());
        let instance = selection.instance_id.to_ascii_lowercase();
        let model = selection.model.to_ascii_lowercase();
        let provider = if driver == Some("antigravity")
            || instance.contains("antigravity")
            || model.starts_with("gemini-")
        {
            Some(crate::commit_message::Provider::Antigravity)
        } else if driver == Some("codex") || instance.contains("codex") {
            Some(crate::commit_message::Provider::Codex)
        } else {
            None
        };
        crate::commit_message::Target {
            provider,
            model: Some(
                if provider == Some(crate::commit_message::Provider::Antigravity) {
                    crate::commit_message::antigravity_cli_model(
                        &selection.model,
                        selection
                            .option_value("thought_level")
                            .or_else(|| choice.and_then(|choice| choice.reasoning_value())),
                    )
                } else {
                    selection.model.clone()
                },
            ),
        }
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
        self.models.reasoning_focus = false;
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
        if let (Some(index), Some(current)) = (index, self.current_model().cloned()) {
            self.models.choices[index].selection.options = current.options;
        }
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

    pub fn move_reasoning(&mut self, forward: bool) {
        if let Some(index) = self.models.list_state.selected()
            && let Some(choice) = self.models.choices.get_mut(index)
        {
            choice.cycle_reasoning(forward);
        }
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
        if self.current_model() == Some(&selection) {
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
        let steer = std::mem::take(&mut self.steer_requested);
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
        if self.paste_pending {
            self.send_notice = Some("Wait for clipboard paste to finish.".into());
            return None;
        }
        if self.draft.trim().is_empty() && self.images.get(&id).is_none_or(Vec::is_empty) {
            self.send_notice = Some("Type a message before sending.".into());
            return None;
        }
        if self.draft_thread.as_deref() != Some(&id) {
            self.send_notice = Some("Thread selection changed. Please try again.".into());
            return None;
        }
        self.previous_messages
            .insert(id.clone(), self.draft.clone());
        if self.usage_exhausted() {
            self.send_notice = Some("Usage limit reached. When usage is available again, press r to resend the previous message.".into());
            return None;
        }
        let images = self.images.get(&id).cloned().unwrap_or_default();
        let fresh_context = self.fresh_context_threads.contains(&id);
        let steer = steer && !fresh_context;
        let mut request = self
            .last_attempt
            .get(&id)
            .filter(|request| {
                request.text == self.draft
                    && request.attachments == images
                    && request.fresh_context == fresh_context
            })
            .cloned()
            .unwrap_or_else(|| SendRequest::new(id.clone(), self.draft.clone()));
        request.attachments = images;
        request.steer = steer;
        request.fresh_context = fresh_context;
        self.last_attempt.insert(id.clone(), request.clone());
        self.previous_messages
            .insert(id.clone(), request.text.clone());
        self.send_status.insert(
            id,
            if steer {
                "Sending steering message…"
            } else {
                "Submitting message…"
            }
            .into(),
        );
        self.send_pending = Some(request.clone());
        Some(request)
    }

    pub fn usage_exhausted(&self) -> bool {
        let Some(model) = self.current_model() else {
            return false;
        };
        ["session", "weekly"].into_iter().any(|kind| {
            self.usage
                .remaining(&model.instance_id, &model.model, kind)
                .is_some_and(|remaining| remaining.percent <= 0.0)
        })
    }

    pub fn retry_previous_message(&mut self) {
        let Some(id) = self.selected_thread_id().map(str::to_owned) else {
            return;
        };
        let Some(message) = self.previous_messages.get(&id).cloned() else {
            return;
        };
        if self.usage_exhausted() {
            self.send_notice = Some("Usage limit reached. When usage is available again, press r to resend the previous message.".into());
            return;
        }
        if self.draft.trim().is_empty() {
            self.draft = message;
            self.draft_cursor = self.draft.chars().count();
            self.draft_thread = Some(id);
        }
        self.send_requested = true;
        self.send_notice = None;
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
                self.fresh_context_threads.remove(&request.thread_id);
                if let Some(images) = self.images.get_mut(&request.thread_id) {
                    images.retain(|image| !request.attachments.contains(image));
                }
                if self.draft_thread.as_deref() == Some(&request.thread_id) {
                    if self.draft == request.text {
                        self.draft.clear();
                        self.draft_cursor = 0;
                    }
                    self.follow_chat = true;
                } else if self.drafts.get(&Some(request.thread_id.clone())) == Some(&request.text) {
                    self.drafts.remove(&Some(request.thread_id.clone()));
                }
                self.draft_history.remove(&Some(request.thread_id.clone()));
                self.last_attempt.remove(&request.thread_id);
                self.send_status.remove(&request.thread_id);
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
        if let Some(id) = &draft_thread {
            self.finished_threads.remove(id);
        }
        if self.draft_thread != draft_thread {
            self.screen_selection = None;
            self.screen_selection_area = None;
            self.screen_selecting = false;
            self.context_menu = None;
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
        if let Some(id) = &selected {
            self.finished_threads.remove(id);
        }
        if selected == self.history_thread {
            return false;
        }
        self.context_usage = None;
        let wrap = self.diff_view.wrap;
        if let Some(previous) = self.history_thread.take() {
            self.thread_diffs.insert(
                previous,
                ThreadDiffs {
                    view: std::mem::take(&mut self.diff_view),
                    counts: std::mem::take(&mut self.run_diffs),
                    requested: std::mem::take(&mut self.diff_requested),
                },
            );
        }
        let cached = selected
            .as_ref()
            .and_then(|id| self.thread_diffs.remove(id))
            .unwrap_or_default();
        self.diff_view = cached.view;
        self.diff_view.wrap = wrap;
        self.run_diffs = cached.counts;
        self.diff_requested = cached.requested;
        self.history_thread = selected;
        self.live_model = None;
        self.live_runtime_mode = None;
        self.access_open = false;
        self.access_error = None;
        self.runtime_requests.clear();
        self.approval_highlight = 0;
        self.approval_dismissed_id = None;
        self.approval_error = None;
        self.history_sequence = None;
        self.history_live = false;
        self.activity.clear();
        self.thinking_since = None;
        self.worked_duration = None;
        self.runs.clear();
        self.conversation_messages.clear();
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
                self.live_runtime_mode = projection.thread.runtime_mode.clone();
                self.context_usage = projection.thread.context_usage;
                if let Some(mode) = projection.thread.runtime_mode {
                    self.set_thread_runtime_mode(thread_id, mode);
                }
                self.runtime_requests = projection
                    .runtime_requests
                    .into_iter()
                    .map(|req| (req.id.clone(), req))
                    .collect();
                for checkpoint in projection.checkpoints {
                    self.apply_checkpoint(checkpoint);
                }
                self.conversation_messages = projection
                    .messages
                    .into_iter()
                    .map(|message| (message.id.clone(), message))
                    .collect();
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
                    "message.updated" => {
                        if let Ok(message) = serde_json::from_value::<
                            crate::history::ConversationMessage,
                        >(event.payload)
                        {
                            self.conversation_messages
                                .insert(message.id.clone(), message);
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
                    | "thread.runtime-mode-updated"
                    | "thread.provider-switched" => {
                        if let Some(selection) = event.payload.get("modelSelection")
                            && let Ok(selection) = serde_json::from_value(selection.clone())
                        {
                            self.set_thread_model(thread_id, selection);
                        }
                        if let Some(mode) = event
                            .payload
                            .get("runtimeMode")
                            .and_then(|value| value.as_str())
                        {
                            self.set_thread_runtime_mode(thread_id, mode.to_owned());
                        }
                        if let Some(title) =
                            event.payload.get("title").and_then(|value| value.as_str())
                            && let Some(history) = &mut self.transcript
                        {
                            history.title = title.into();
                        }
                    }
                    "runtime-request.updated" => {
                        if let Ok(req) =
                            serde_json::from_value::<crate::history::RuntimeRequest>(event.payload)
                        {
                            if req.status == "pending"
                                && self.approval_dismissed_id.as_deref() != Some(req.id.as_str())
                            {
                                self.approval_highlight = 0;
                                self.approval_error = None;
                            }
                            self.runtime_requests.insert(req.id.clone(), req);
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

    /// Resolve deduplicated text and queued messages from T3's message records.
    pub fn display_transcript(&self) -> Option<crate::history::Transcript> {
        let mut history = self.transcript.clone()?;
        for entry in &mut history.items {
            if let Some(message) = entry
                .item
                .message_id
                .as_ref()
                .and_then(|id| self.conversation_messages.get(id))
            {
                if entry.item.text.as_deref().is_none_or(str::is_empty) {
                    entry.item.text = Some(message.text.clone());
                }
                if entry.item.run_id.is_none() {
                    entry.item.run_id = message.run_id.clone().or_else(|| {
                        self.runs
                            .values()
                            .find(|run| run.user_message_id.as_deref() == Some(message.id.as_str()))
                            .map(|run| run.id.clone())
                    });
                }
            }
        }
        let mut queued: Vec<_> = self
            .runs
            .values()
            .filter(|run| run.status == "queued")
            .collect();
        queued.sort_by_key(|run| {
            (
                run.queue_position.or(run.ordinal).unwrap_or(u64::MAX),
                run.id.clone(),
            )
        });
        let mut position = history
            .items
            .iter()
            .map(|entry| entry.position)
            .max()
            .unwrap_or(0);
        for run in queued {
            let Some(message) = run
                .user_message_id
                .as_ref()
                .and_then(|id| self.conversation_messages.get(id))
                .or_else(|| {
                    self.conversation_messages.values().find(|message| {
                        message.role == "user" && message.run_id.as_deref() == Some(run.id.as_str())
                    })
                })
            else {
                continue;
            };
            if history.items.iter().any(|entry| {
                entry.item.kind == "user_message"
                    && (entry.item.message_id.as_deref() == Some(message.id.as_str())
                        || entry.item.run_id.as_deref() == Some(run.id.as_str()))
            }) {
                continue;
            }
            position = position.saturating_add(1);
            let item = serde_json::from_value(serde_json::json!({
                "id": message.id, "messageId": message.id, "type":"user_message",
                "runId":run.id, "text":message.text, "attachments":message.attachments
            }))
            .expect("valid message projection");
            history.items.push(crate::history::ProjectedItem {
                position,
                source_item_id: None,
                item,
            });
        }
        Some(history)
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
        if self.screen_selection_area == Some(self.chat_text_area) {
            self.screen_selection = None;
            self.screen_selection_area = None;
            self.screen_selecting = false;
        }
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

    pub fn selected_project_id(&self) -> Option<String> {
        self.selected_location().map(|loc| loc.project_id)
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
        let selected_id = self.selected_thread_id().map(str::to_owned);
        for thread in &catalog.threads {
            if thread.is_running() {
                self.known_running_threads.insert(thread.id.clone());
                self.finished_threads.remove(&thread.id);
            } else if self.known_running_threads.remove(&thread.id)
                && selected_id.as_deref() != Some(thread.id.as_str())
            {
                self.finished_threads.insert(thread.id.clone());
            }
        }
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

    pub fn request_quit(&mut self) {
        let other_running = self.check_other_instances.is_some_and(|check| check());
        if self.local_server_pid.is_some() && !other_running {
            self.quit_dialog = Some(QuitDialog {
                shutdown_server: false,
            });
        } else {
            self.quit = true;
        }
    }

    pub fn open_thread_menu(&mut self) {
        if let Some(Selection::Project(project_id)) = self.selection() {
            let project_id = project_id.clone();
            let title = self
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.projects.iter().find(|p| p.id == project_id))
                .map(|p| p.title.clone())
                .unwrap_or_default();
            self.thread_menu = Some(ThreadMenu {
                thread_id: project_id,
                original_title: title,
                mode: ThreadMenuMode::ProjectOptions,
                selected_option: 0,
                rename_input: String::new(),
                busy: false,
                error: None,
            });
            return;
        }
        let Some(Selection::Thread(thread_id)) = self.selection() else {
            return;
        };
        let thread_id = thread_id.clone();
        let title = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.threads.iter().find(|t| t.id == thread_id))
            .map(|thread| thread.title.clone())
            .unwrap_or_else(|| "Thread".into());
        self.thread_menu = Some(ThreadMenu {
            thread_id,
            original_title: title.clone(),
            mode: ThreadMenuMode::Options,
            selected_option: 0,
            rename_input: title,
            busy: false,
            error: None,
        });
    }

    pub fn finish_thread_action(
        &mut self,
        action: crate::t3::ThreadAction,
        result: Result<(), String>,
    ) {
        match result {
            Ok(()) => {
                self.thread_menu = None;
                match action {
                    crate::t3::ThreadAction::Create {
                        project_id,
                        thread_id,
                        ..
                    } => {
                        self.restore_location = Some(crate::preferences::Location {
                            project_id,
                            thread_id: Some(thread_id),
                        });
                        self.send_notice = Some("Thread created.".into());
                    }
                    crate::t3::ThreadAction::Rename { thread_id, title } => {
                        let title = title.trim().to_owned();
                        if let Some(catalog) = self.catalog.as_mut() {
                            if let Some(thread) =
                                catalog.threads.iter_mut().find(|t| t.id == thread_id)
                            {
                                thread.title = title.clone();
                            }
                            self.rows = catalog.rows();
                        }
                        if self.history_thread.as_deref() == Some(thread_id.as_str())
                            && let Some(transcript) = self.transcript.as_mut()
                        {
                            transcript.title = title;
                        }
                        self.send_notice = Some("Thread renamed.".into());
                    }
                    crate::t3::ThreadAction::Delete { thread_id } => {
                        self.drafts.remove(&Some(thread_id.clone()));
                        self.draft_history.remove(&Some(thread_id.clone()));
                        self.images.remove(&thread_id);
                        if self.history_thread.as_deref() == Some(thread_id.as_str()) {
                            self.history_thread = None;
                            self.transcript = None;
                            self.runs.clear();
                            self.conversation_messages.clear();
                            self.activity.clear();
                            self.history_error = None;
                            self.history_sequence = None;
                            self.history_live = false;
                            self.chat_revision = self.chat_revision.wrapping_add(1);
                            self.chat_cache = None;
                        }
                        if self.draft_thread.as_deref() == Some(thread_id.as_str()) {
                            self.draft_thread = None;
                            self.draft.clear();
                            self.draft_cursor = 0;
                            self.draft_selection_anchor = None;
                        }
                        if let Some(mut catalog) = self.catalog.take() {
                            catalog.threads.retain(|t| t.id != thread_id);
                            self.update_catalog(catalog);
                        }
                        self.send_notice = Some("Thread deleted.".into());
                    }
                }
            }
            Err(error) => {
                if let Some(menu) = self.thread_menu.as_mut() {
                    menu.busy = false;
                    menu.error = Some(error);
                } else {
                    self.send_notice = Some(error);
                }
            }
        }
    }

    pub fn confirm_quit(&mut self, shutdown_server: bool) {
        self.shutdown_server_on_quit = shutdown_server;
        self.quit_dialog = None;
        self.quit = true;
    }

    pub fn server_to_shutdown(&self) -> Option<u32> {
        self.shutdown_server_on_quit
            .then_some(self.local_server_pid)
            .flatten()
    }

    pub fn thread_finished(&self, thread_id: &str) -> bool {
        self.finished_threads.contains(thread_id)
    }

    pub fn is_selected_thread_working(&self) -> bool {
        self.thinking_since.is_some()
            || self
                .selected_thread_id()
                .is_some_and(|id| self.is_thread_running(id))
    }

    pub fn is_thread_running(&self, thread_id: &str) -> bool {
        if self.history_thread.as_deref() == Some(thread_id) && self.transcript.is_some() {
            return self.runs.values().any(|run| run.active());
        }
        self.catalog.as_ref().is_some_and(|catalog| {
            catalog
                .threads
                .iter()
                .any(|thread| thread.id == thread_id && thread.is_running())
        })
    }

    pub fn thread_needs_approval(&self, thread_id: &str) -> bool {
        if self.history_thread.as_deref() == Some(thread_id) && self.transcript.is_some() {
            return self.pending_approval().is_some();
        }
        self.catalog.as_ref().is_some_and(|catalog| {
            catalog
                .threads
                .iter()
                .any(|thread| thread.id == thread_id && thread.needs_approval())
        })
    }

    pub fn current_runtime_mode(&self) -> Option<&str> {
        let id = self.selected_thread_id()?;
        if self.history_thread.as_deref() == Some(id)
            && let Some(mode) = &self.live_runtime_mode
        {
            return Some(mode.as_str());
        }
        self.catalog.as_ref().and_then(|catalog| {
            catalog
                .threads
                .iter()
                .find(|thread| thread.id == id)
                .and_then(|thread| thread.runtime_mode.as_deref())
        })
    }

    pub fn set_thread_runtime_mode(&mut self, thread_id: &str, mode: String) {
        if self.history_thread.as_deref() == Some(thread_id) {
            self.live_runtime_mode = Some(mode.clone());
        }
        if let Some(catalog) = &mut self.catalog
            && let Some(thread) = catalog
                .threads
                .iter_mut()
                .find(|thread| thread.id == thread_id)
        {
            thread.runtime_mode = Some(mode);
        }
    }

    pub fn open_access_picker(&mut self) {
        self.access_error = None;
        self.access_highlight = self
            .current_runtime_mode()
            .and_then(|current| RUNTIME_MODES.iter().position(|(id, _, _)| *id == current))
            .unwrap_or(0);
        self.access_open = true;
    }

    pub fn confirm_access_picker(&mut self) {
        let Some(thread_id) = self.selected_thread_id().map(str::to_owned) else {
            self.access_error = Some("Select a thread first.".into());
            return;
        };
        let Some((mode, _, _)) = RUNTIME_MODES.get(self.access_highlight) else {
            return;
        };
        let request = crate::t3::RuntimeModeChange {
            command_id: uuid::Uuid::new_v4().to_string(),
            thread_id,
            runtime_mode: (*mode).to_owned(),
        };
        self.access_error = None;
        self.access_pending = Some(request.clone());
        self.access_requested = Some(request);
        self.access_open = false;
    }

    pub fn apply_runtime_mode_change(
        &mut self,
        request: crate::t3::RuntimeModeChange,
        result: Result<(), String>,
    ) {
        if self
            .access_pending
            .as_ref()
            .is_some_and(|pending| pending.command_id == request.command_id)
        {
            self.access_pending = None;
        }
        match result {
            Ok(()) => {
                self.set_thread_runtime_mode(&request.thread_id, request.runtime_mode);
                self.access_error = None;
            }
            Err(error) => {
                self.access_error = Some(error);
            }
        }
    }

    pub fn pending_approval(&self) -> Option<PendingApproval> {
        let transcript = self.transcript.as_ref()?;
        let mut items: Vec<_> = transcript
            .items
            .iter()
            .filter(|entry| entry.item.kind == "approval_request")
            .collect();
        items.sort_by_key(|entry| entry.position);
        for entry in items {
            let item = &entry.item;
            let request_id = item
                .request_id
                .as_deref()
                .or(item.id.as_deref())
                .unwrap_or_default();
            if request_id.is_empty() {
                continue;
            }
            let req = self.runtime_requests.get(request_id);
            let is_pending = req.map_or_else(
                || {
                    item.status
                        .as_deref()
                        .is_none_or(|status| matches!(status, "pending" | "waiting"))
                },
                |r| r.status == "pending",
            );
            if !is_pending {
                continue;
            }
            let options = if item.options.is_empty() {
                default_approval_options()
            } else {
                item.options.clone()
            };
            return Some(PendingApproval {
                request_id: request_id.to_owned(),
                kind: item
                    .request_kind
                    .clone()
                    .or_else(|| req.map(|r| r.kind.clone()))
                    .unwrap_or_else(|| "permission".into()),
                prompt: item
                    .prompt
                    .clone()
                    .or_else(|| item.title.clone())
                    .or_else(|| item.text.clone()),
                reason: item.reason.clone(),
                app_name: item.app_name.clone(),
                options,
            });
        }
        // Fallback: if runtime_requests has a pending approval without a projected turn item yet
        let mut reqs: Vec<_> = self
            .runtime_requests
            .values()
            .filter(|r| {
                r.status == "pending"
                    && matches!(
                        r.kind.as_str(),
                        "command" | "file-read" | "file-change" | "mcp-elicitation" | "permission"
                    )
            })
            .collect();
        reqs.sort_by(|a, b| a.id.cmp(&b.id));
        reqs.into_iter().next().map(|r| PendingApproval {
            request_id: r.id.clone(),
            kind: r.kind.clone(),
            prompt: None,
            reason: None,
            app_name: None,
            options: default_approval_options(),
        })
    }

    pub fn active_approval_popup(&self) -> Option<PendingApproval> {
        let pending = self.pending_approval()?;
        if self.approval_dismissed_id.as_deref() == Some(pending.request_id.as_str()) {
            return None;
        }
        Some(pending)
    }

    pub fn confirm_approval(&mut self) {
        let Some(thread_id) = self.selected_thread_id().map(str::to_owned) else {
            return;
        };
        let Some(pending) = self.active_approval_popup() else {
            return;
        };
        if self.approval_pending.is_some() {
            return;
        }
        let index = self
            .approval_highlight
            .min(pending.options.len().saturating_sub(1));
        let Some(option) = pending.options.get(index) else {
            return;
        };
        let request = crate::t3::ApprovalResponse {
            command_id: uuid::Uuid::new_v4().to_string(),
            thread_id,
            request_id: pending.request_id,
            decision: option.decision.clone(),
        };
        self.approval_error = None;
        self.approval_pending = Some(request.clone());
        self.approval_requested = Some(request);
    }

    pub fn apply_approval_response(
        &mut self,
        request: crate::t3::ApprovalResponse,
        result: Result<(), String>,
    ) {
        if self
            .approval_pending
            .as_ref()
            .is_some_and(|pending| pending.command_id == request.command_id)
        {
            self.approval_pending = None;
        }
        match result {
            Ok(()) => {
                if let Some(entry) = self.runtime_requests.get_mut(&request.request_id) {
                    entry.status = "resolved".into();
                    entry.decision = Some(request.decision);
                }
                self.approval_error = None;
                self.approval_highlight = 0;
            }
            Err(error) => {
                self.approval_error = Some(error);
            }
        }
    }

    pub fn apply_layout_preset(&mut self, preset: crate::preferences::LayoutPreset) {
        self.layout_preset = preset;
        self.layout_highlight = preset;
        self.workspace_width = preset.workspace_width();
        self.workspace_visible = preset.workspace_visible();
        if !self.workspace_visible && matches!(self.focus, Focus::Workspace | Focus::WorkspaceTabs) {
            self.focus = Focus::Chat;
        }
    }
}

fn default_approval_options() -> Vec<crate::history::ApprovalOption> {
    vec![
        crate::history::ApprovalOption {
            decision: "accept".into(),
            label: "Approve once".into(),
            warning: None,
        },
        crate::history::ApprovalOption {
            decision: "acceptForSession".into(),
            label: "Approve for session".into(),
            warning: None,
        },
        crate::history::ApprovalOption {
            decision: "decline".into(),
            label: "Decline".into(),
            warning: None,
        },
        crate::history::ApprovalOption {
            decision: "cancel".into(),
            label: "Cancel".into(),
            warning: None,
        },
    ]
}

fn normalize_draft_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .chars()
        .filter(|ch| *ch == '\n' || !ch.is_control())
        .collect()
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
    fn selected_draft_copies_and_replaces_unicode_text() {
        let mut app = App::default();
        app.insert_draft("a世界z");
        app.draft_selection_anchor = Some(1);
        app.draft_cursor = 3;
        assert_eq!(app.selected_draft().as_deref(), Some("世界"));
        app.insert_draft("hi");
        assert_eq!(app.draft, "ahiz");
        assert_eq!(app.draft_cursor, 3);
        assert!(app.selected_draft().is_none());
    }

    #[test]
    fn selected_screen_reads_visible_rows() {
        let mut app = App::default();
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 5, 2));
        buffer.set_string(0, 0, "hello", ratatui::style::Style::default());
        buffer.set_string(0, 1, "world", ratatui::style::Style::default());
        app.screen_buffer = Some(buffer);
        app.screen_selection = Some(((0, 2), (1, 3)));
        assert_eq!(app.selected_screen().as_deref(), Some("llo\nwor"));
    }

    #[test]
    fn chat_selection_crosses_wrapped_and_real_lines_without_neighboring_panes() {
        use ratatui::{
            layout::Rect,
            text::{Line, Text},
        };
        let mut app = App::default();
        let chat = Rect::new(5, 0, 8, 3);
        let viewport = crate::chat::Viewport::new(
            Text::from(vec![Line::from("│ abcdefghijkl"), Line::from("│ next")]),
            chat.width,
        );
        let mut screen = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 20, 3));
        for y in 0..chat.height {
            for x in 0..chat.width {
                screen[(chat.x + x, y)] = viewport.buffer[(x, y)].clone();
            }
            screen.set_string(13, y, "OTHER", ratatui::style::Style::default());
        }
        app.chat_viewport = Some(viewport);
        app.chat_text_area = chat;
        app.screen_selection_area = Some(chat);
        app.screen_selection = Some(((0, 7), (2, 10)));
        app.screen_buffer = Some(screen);
        assert_eq!(app.selected_screen().as_deref(), Some("abcdefghijkl\nnex"));
    }

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
    fn diff_results_survive_switching_threads_even_when_they_arrive_late() {
        let mut app = App {
            authenticated: true,
            ..Default::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.diff_requested.insert("r1".into());
        app.move_selection(true);
        app.sync_history_selection();
        app.finish_diff(
            "t1".into(),
            "r1".into(),
            Ok("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -0,0 +1 @@\n+hello\n".into()),
        );
        assert!(!app.diff_view.patches.contains_key("r1"));
        app.move_selection(false);
        app.sync_history_selection();
        assert!(app.diff_view.patches.contains_key("r1"));
        assert!(app.diff_requested.contains("r1"));
        assert_eq!(app.run_diffs["r1"]["a"], (1, 0));
    }

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
    fn draft_undo_restores_unicode_cursor_selection_and_invalidates_redo_on_edit() {
        let mut app = App::default();
        app.insert_draft("hello 世界");
        app.draft_cursor = 8;
        app.draft_selection_anchor = Some(6);
        app.insert_draft("👩‍💻\npasted");
        let replaced = app.draft.clone();
        app.undo_draft(false);
        assert_eq!(app.draft, "hello 世界");
        assert_eq!(app.draft_cursor, 8);
        assert_eq!(app.draft_selection_anchor, Some(6));
        app.undo_draft(true);
        assert_eq!(app.draft, replaced);
        app.backspace_draft_word();
        app.undo_draft(false);
        assert_eq!(app.draft, replaced);
        app.insert_draft("!");
        app.undo_draft(true);
        assert_eq!(app.draft, format!("{replaced}!"));
        app.clear_draft();
        app.undo_draft(false);
        assert_eq!(app.draft, format!("{replaced}!"));
    }

    #[test]
    fn draft_undo_is_per_thread_and_cannot_restore_a_sent_message() {
        let mut app = ready();
        app.draft.clear();
        app.draft_cursor = 0;
        app.insert_draft("First draft");
        app.move_selection(true);
        app.sync_history_selection();
        app.insert_draft("Second draft");
        app.undo_draft(false);
        assert!(app.draft.is_empty());
        app.move_selection(false);
        app.sync_history_selection();
        assert_eq!(app.draft, "First draft");
        app.undo_draft(false);
        assert!(app.draft.is_empty());
        app.undo_draft(true);
        let request = app.prepare_send().unwrap();
        app.finish_send(request, Ok(()));
        app.undo_draft(false);
        assert!(app.draft.is_empty());
        app.undo_draft(true);
        assert!(app.draft.is_empty());
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

    #[test]
    fn dictation_lifecycle_inserts_transcript_with_spacing() {
        use crate::dictation::{Command, Event, Phase};
        let mut app = ready();
        app.draft = "Hello".into();
        app.draft_cursor = 5;
        app.dictation.toggle_requested = true;
        assert_eq!(app.take_dictation_command(), Some(Command::Start));
        assert_eq!(app.dictation.phase, Phase::Starting);

        app.apply_dictation(Event::Recording {
            program: "parecord",
        });
        assert_eq!(app.dictation.phase, Phase::Recording);

        app.dictation.toggle_requested = true;
        assert_eq!(app.take_dictation_command(), Some(Command::Stop));

        app.apply_dictation(Event::Transcribing);
        assert_eq!(app.dictation.phase, Phase::Transcribing);

        app.apply_dictation(Event::Finished(Ok("world".into())));
        assert_eq!(app.dictation.phase, Phase::Idle);
        assert_eq!(app.draft, "Hello world");
    }

    #[test]
    fn server_version_comparison_and_update_actions() {
        assert!(is_newer_version("0.1.10", "0.1.11"));
        assert!(is_newer_version("v0.1.10", "0.2.0"));
        assert!(is_newer_version("0.1.10-nightly.1", "0.1.10"));
        assert!(!is_newer_version("0.1.10", "0.1.10"));
        assert!(!is_newer_version("0.1.10", "0.1.9"));
        assert!(!is_newer_version("0.1.10", "0.1.10-nightly.1"));

        let mut app = App::default();
        app.authenticated = true;
        app.server_version = Some("0.1.10".into());
        app.apply_server_version_check(Ok(Some(crate::t3::NpmRelease {
            version: "0.1.10".into(),
            database_migration: None,
        })));
        assert_eq!(
            app.server_version_status,
            ServerVersionStatus::UpToDate {
                latest: "0.1.10".into()
            }
        );

        app.trigger_server_update_action();
        assert_eq!(app.server_version_status, ServerVersionStatus::Checking);
        assert!(app.server_version_check_requested);

        app.server_version_check_requested = false;
        app.apply_server_version_check(Ok(Some(crate::t3::NpmRelease {
            version: "0.1.11".into(),
            database_migration: Some(3),
        })));
        assert_eq!(
            app.server_version_status,
            ServerVersionStatus::UpdateAvailable {
                latest: "0.1.11".into(),
                database_migration: Some(3),
            }
        );

        app.trigger_server_update_action();
        assert!(app.server_update_requested);
        assert!(matches!(
            app.server_update_status,
            ServerUpdateStatus::Downloading {
                ref target_version,
                ..
            } if target_version == "0.1.11"
        ));
    }
}
