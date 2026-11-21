//! Small T3-backed branch / commit / push panel.
use crate::{
    app::{App, Focus},
    catalog::Selection,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub enum Operation {
    Generate(String),
    Refresh,
    Switch(String),
    Commit(String),
    CommitPush(String),
    Push,
}
#[derive(Clone)]
pub struct Request {
    pub id: String,
    pub cwd: String,
    pub operation: Operation,
    pub generator: crate::commit_message::Target,
}
impl Request {
    pub fn payload(&self) -> Value {
        match &self.operation {
            Operation::Switch(branch) => json!({"cwd":self.cwd,"refName":branch}),
            Operation::Commit(message) => {
                json!({"actionId":self.id,"cwd":self.cwd,"action":"commit","commitMessage":message,"featureBranch":false})
            }
            Operation::CommitPush(message) => {
                json!({"actionId":self.id,"cwd":self.cwd,"action":"commit_push","commitMessage":message,"featureBranch":false})
            }
            Operation::Push => {
                json!({"actionId":self.id,"cwd":self.cwd,"action":"push","featureBranch":false})
            }
            Operation::Refresh | Operation::Generate(_) => json!({"cwd":self.cwd}),
        }
    }
}
pub struct Snapshot {
    pub generated_message: Option<String>,
    pub status: Status,
    pub branches: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub is_repo: bool,
    pub ref_name: Option<String>,
    pub has_primary_remote: bool,
    pub ahead_count: u64,
    pub behind_count: u64,
    pub working_tree: WorkingTree,
}
#[derive(Deserialize)]
pub struct WorkingTree {
    pub files: Vec<File>,
}
#[derive(Deserialize)]
pub struct File {
    pub path: String,
    pub insertions: u64,
    pub deletions: u64,
}

pub const MODES: [&str; 3] = ["Commit & Push", "Commit", "Push"];

#[derive(Default)]
pub struct GitState {
    pub cwd: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub pending: Option<String>,
    pub request: Option<Request>,
    pub notice: String,
    pub error: Option<String>,
    pub row: usize,
    pub commit_modal: bool,
    pub commit_push: bool,
    pub commit_row: usize,
    pub message: String,
    pub message_scroll: usize,
    pub message_follow_end: bool,
    pub message_area: Rect,
    pub editing: bool,
    pub picking: bool,
    pub picking_mode: bool,
    pub mode: usize,
    pub mode_index: usize,
    pub files_scroll: usize,
    pub files_area: Rect,
    pub branch_index: usize,
    pub confirm: Option<Operation>,
    pub refreshed: Option<std::time::Instant>,
    pub generator: crate::commit_message::Target,
}
impl GitState {
    pub fn has_modal(&self) -> bool {
        self.commit_modal || self.picking || self.picking_mode || self.confirm.is_some()
    }
    pub fn select_mode(&mut self, index: usize) {
        if index >= MODES.len() || self.pending.is_some() {
            return;
        }
        self.mode = index;
        self.picking_mode = false;
        self.error = None;
        if index == 2 {
            if !self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.has_primary_remote)
            {
                self.error = Some("No remote configured.".into());
            } else {
                self.confirm = Some(Operation::Push);
            }
        } else {
            self.commit_modal = true;
            self.commit_push = index == 0;
            self.commit_row = 0;
            self.message_scroll = 0;
            self.files_scroll = 0;
            self.message_follow_end = false;
            self.editing = false;
        }
    }

    fn submit_commit(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let error = if self.message.trim().is_empty() {
            Some("Enter a commit message first.")
        } else if !self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status.is_repo)
        {
            Some("Not a Git repository.")
        } else if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status.working_tree.files.is_empty())
        {
            Some("No changes to commit.")
        } else if self.commit_push
            && !self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.has_primary_remote)
        {
            Some("No remote configured.")
        } else {
            None
        };
        if let Some(error) = error {
            self.error = Some(error.into());
            return;
        }
        self.editing = false;
        self.queue(if self.commit_push {
            Operation::CommitPush(self.message.trim().into())
        } else {
            Operation::Commit(self.message.trim().into())
        });
        if self.pending.is_some() {
            self.commit_modal = false;
        }
    }

    pub fn scroll_message(&mut self, down: bool, rows: usize) {
        self.message_follow_end = false;
        self.message_scroll = if down {
            self.message_scroll.saturating_add(rows)
        } else {
            self.message_scroll.saturating_sub(rows)
        };
    }

    pub fn queue(&mut self, operation: Operation) {
        if self.pending.is_some() {
            return;
        }
        if let Some(cwd) = &self.cwd {
            let id = uuid::Uuid::new_v4().to_string();
            self.pending = Some(id.clone());
            self.error = None;
            self.notice = if matches!(operation, Operation::Generate(_)) {
                "Generating message…"
            } else {
                "Working…"
            }
            .into();
            self.request = Some(Request {
                id,
                cwd: cwd.clone(),
                operation,
                generator: self.generator.clone(),
            });
        } else {
            self.error =
                Some("Select a project or thread with an available workspace first.".into());
        }
    }
    pub fn finish(&mut self, request: &Request, result: Result<Snapshot, String>) {
        if self.pending.as_deref() != Some(&request.id) || self.cwd.as_deref() != Some(&request.cwd)
        {
            return;
        }
        self.pending = None;
        self.refreshed = Some(std::time::Instant::now());
        match result {
            Ok(snapshot) => {
                if let Operation::Generate(before) = &request.operation
                    && self.message == *before
                {
                    if let Some(message) = &snapshot.generated_message {
                        self.message = message.clone();
                        self.message_scroll = 0;
                        self.message_follow_end = false;
                    } else {
                        self.error = Some("No changes to describe.".into());
                    }
                }
                if matches!(&request.operation, Operation::Commit(text) | Operation::CommitPush(text) if text == self.message.trim())
                {
                    self.message.clear();
                    self.commit_modal = false;
                    self.editing = false;
                }
                self.notice = match request.operation {
                    Operation::Refresh => String::new(),
                    Operation::Generate(_) if self.error.is_some() => String::new(),
                    Operation::Generate(_) => {
                        "Generated an AI message from the diff. Review before committing.".into()
                    }
                    Operation::Switch(_) => "Branch switched.".into(),
                    Operation::Commit(_) => "Commit finished.".into(),
                    Operation::CommitPush(_) => "Commit and push finished.".into(),
                    Operation::Push => "Push finished.".into(),
                };
                self.snapshot = Some(snapshot);
            }
            Err(error) => {
                self.error = Some(error);
                self.notice.clear();
            }
        }
    }
}

pub fn workspace(app: &App) -> Option<String> {
    let catalog = app.catalog.as_ref()?;
    let project = match app.selection()? {
        Selection::Project(id) => id.as_str(),
        Selection::Thread(id) => {
            let thread = catalog.threads.iter().find(|thread| &thread.id == id)?;
            if let Some(path) = &thread.worktree_path {
                return Some(path.clone());
            }
            thread.project_id.as_str()
        }
    };
    catalog
        .projects
        .iter()
        .find(|entry| entry.id == project)
        .map(|entry| entry.workspace_root.clone())
}

pub fn sync(app: &mut App) {
    let cwd = workspace(app);
    let generator = app.commit_generator_target();
    if app.git.cwd != cwd {
        app.git = GitState {
            cwd,
            generator: generator.clone(),
            ..Default::default()
        };
    }
    app.git.generator = generator;
    if matches!(app.workspace_tab, 0 | 3)
        && app.authenticated
        && app.git.pending.is_none()
        && !app.git.has_modal()
        && !app.git.editing
        && (app.git.snapshot.is_none() && app.git.refreshed.is_none()
            || app
                .git
                .refreshed
                .is_some_and(|time| time.elapsed().as_secs() >= 10))
    {
        app.git.queue(Operation::Refresh);
    }
}

pub fn key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind == crossterm::event::KeyEventKind::Release {
        return true;
    }
    if key.code == KeyCode::Enter && key.kind == crossterm::event::KeyEventKind::Repeat {
        return true;
    }
    if key.code == KeyCode::Char('p') && !app.authenticated && !app.git.has_modal() {
        if !app.pairing_in_progress {
            app.pairing_requested = true;
        }
        return true;
    }
    let git = &mut app.git;
    if let Some(operation) = git.confirm.take() {
        let submit = if matches!(operation, Operation::Push) {
            key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL)
        } else {
            matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
        };
        if submit && git.pending.is_none() {
            git.queue(operation);
        } else if key.code != KeyCode::Esc {
            git.confirm = Some(operation);
        }
        return true;
    }
    if git.picking_mode {
        match key.code {
            KeyCode::Esc => git.picking_mode = false,
            KeyCode::Down | KeyCode::Char('j') => {
                git.mode_index = (git.mode_index + 1).min(MODES.len() - 1)
            }
            KeyCode::Up | KeyCode::Char('k') => git.mode_index = git.mode_index.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => git.select_mode(git.mode_index),
            _ => {}
        }
        return true;
    }
    if git.picking {
        let count = git
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.branches.len());
        match key.code {
            KeyCode::Esc => git.picking = false,
            KeyCode::Down | KeyCode::Char('j') => {
                git.branch_index = (git.branch_index + 1).min(count.saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                git.branch_index = git.branch_index.saturating_sub(1)
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(branch) = git
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.branches.get(git.branch_index))
                {
                    git.confirm = Some(Operation::Switch(branch.clone()));
                }
                git.picking = false;
            }
            _ => {}
        }
        return true;
    }
    if git.commit_modal || git.editing {
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            git.submit_commit();
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
            git.queue(Operation::Generate(git.message.clone()));
            return true;
        }
        match key.code {
            KeyCode::PageUp | KeyCode::PageDown => {
                let down = key.code == KeyCode::PageDown;
                if git.commit_row == 1 {
                    git.files_scroll = if down {
                        git.files_scroll
                            .saturating_add(git.files_area.height.max(1) as usize)
                    } else {
                        git.files_scroll
                            .saturating_sub(git.files_area.height.max(1) as usize)
                    };
                } else {
                    git.scroll_message(down, git.message_area.height.max(1) as usize);
                }
                return true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                git.commit_row = 1 - git.commit_row.min(1);
                git.editing = false;
                return true;
            }
            KeyCode::Home | KeyCode::End if !git.editing => {
                let value = if key.code == KeyCode::Home {
                    0
                } else {
                    usize::MAX
                };
                if git.commit_row == 1 {
                    git.files_scroll = value;
                } else {
                    git.message_scroll = value;
                    git.message_follow_end = false;
                }
                return true;
            }
            _ => {}
        }
        if git.editing {
            match key.code {
                KeyCode::Esc => git.editing = false,
                KeyCode::Enter => {
                    git.message.push('\n');
                    git.message_follow_end = true;
                }
                KeyCode::Backspace => {
                    use unicode_segmentation::UnicodeSegmentation;
                    if let Some((byte, _)) = git.message.grapheme_indices(true).next_back() {
                        git.message.truncate(byte);
                    }
                    git.message_follow_end = true;
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    git.message.push(ch);
                    git.message_follow_end = true;
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Esc => git.commit_modal = false,
                KeyCode::Char('g') => git.queue(Operation::Generate(git.message.clone())),
                KeyCode::Enter | KeyCode::Char(' ' | 'i') => {
                    git.commit_row = 0;
                    git.editing = true;
                    git.message_follow_end = true;
                }
                KeyCode::Up | KeyCode::Char('k') if git.commit_row == 1 => {
                    git.files_scroll = git.files_scroll.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') if git.commit_row == 1 => {
                    git.files_scroll = git.files_scroll.saturating_add(1)
                }
                _ => {}
            }
        }
        return true;
    }
    if git.pending.is_some() {
        return false;
    }
    match key.code {
        KeyCode::Char('g') => git.queue(Operation::Generate(git.message.clone())),
        KeyCode::Char('r') => git.queue(Operation::Refresh),
        KeyCode::Up | KeyCode::Char('k') if git.row > 0 => git.row -= 1,
        KeyCode::Down | KeyCode::Char('j') => git.row = 1,
        KeyCode::Enter | KeyCode::Char(' ') => {
            if !git
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.is_repo)
            {
                return true;
            }
            if git.row == 0 {
                git.picking = true;
                git.branch_index = git
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        snapshot.branches.iter().position(|name| {
                            Some(name.as_str()) == snapshot.status.ref_name.as_deref()
                        })
                    })
                    .unwrap_or(0);
            } else {
                git.picking_mode = true;
                git.mode_index = git.mode;
            }
        }
        _ => return false,
    }
    true
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let git = &app.git;
    let inner = area;
    let branch = git
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.status.ref_name.as_deref())
        .unwrap_or("detached / unknown");
    let [controls, details] =
        Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).areas(inner);
    let rows = Layout::vertical([Constraint::Length(3); 2]).split(controls);
    let labels = [branch.to_owned(), MODES[git.mode.min(2)].to_owned()];
    for (index, rect) in rows.iter().enumerate() {
        let block = crate::ui::panel(
            if index == 0 { " Branch " } else { " Mode " },
            app.focus == Focus::Workspace && git.row == index,
        );
        let inner = block.inner(*rect);
        frame.render_widget(block, *rect);
        let [label, arrow] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(2)]).areas(inner);
        frame.render_widget(Paragraph::new(labels[index].as_str()), label);
        frame.render_widget(
            Paragraph::new("▾").style(Style::default().fg(Color::Cyan)),
            arrow,
        );
        app.click_targets
            .push((*rect, crate::app::ClickTarget::GitControl(index)));
    }
    let mut lines = vec![Line::styled(
        git.cwd
            .clone()
            .unwrap_or("Select a project or thread.".into()),
        Style::default().fg(if git.cwd.is_some() {
            Color::Gray
        } else {
            Color::DarkGray
        }),
    )];
    if let Some(snapshot) = &git.snapshot {
        if !snapshot.status.is_repo {
            lines.push(Line::styled(
                "Not a Git repository.",
                Style::default().fg(Color::Yellow),
            ));
        } else {
            let ahead_color = if snapshot.status.ahead_count > 0 {
                Color::Green
            } else {
                Color::DarkGray
            };
            let behind_color = if snapshot.status.behind_count > 0 {
                Color::Yellow
            } else {
                Color::DarkGray
            };
            let changed_count = snapshot.status.working_tree.files.len();
            let changed_color = if changed_count > 0 {
                Color::Cyan
            } else {
                Color::DarkGray
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("↑{} ahead", snapshot.status.ahead_count),
                    Style::default().fg(ahead_color),
                ),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("↓{} behind", snapshot.status.behind_count),
                    Style::default().fg(behind_color),
                ),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{changed_count} changed files"),
                    Style::default().fg(changed_color),
                ),
            ]));
            for file in snapshot
                .status
                .working_tree
                .files
                .iter()
                .take(details.height.saturating_sub(5) as usize)
            {
                lines.push(Line::from(vec![
                    Span::styled(file.path.clone(), Style::default().fg(Color::White)),
                    Span::raw(" "),
                    Span::styled(
                        format!("+{}", file.insertions),
                        Style::default().fg(Color::Green),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("-{}", file.deletions),
                        Style::default().fg(Color::Red),
                    ),
                ]));
            }
        }
    }
    if let Some(error) = &git.error {
        lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    }
    if !git.notice.is_empty() {
        lines.push(Line::styled(
            git.notice.clone(),
            Style::default().fg(Color::Cyan),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        details,
    );
}

pub fn draw_modal(frame: &mut Frame, app: &mut App) {
    if app.git.picking_mode {
        let bounds = frame.area();
        let width = bounds.width.min(44);
        let height = bounds.height.min(7);
        let area = Rect::new(
            bounds.x + (bounds.width - width) / 2,
            bounds.y + (bounds.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block =
            crate::ui::panel(" Select mode ", true).title_bottom(" Enter: select · Esc: back ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        for (index, label) in MODES.iter().enumerate().take(inner.height as usize) {
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            frame.render_widget(
                Paragraph::new(*label).style(if app.git.mode_index == index {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                }),
                row,
            );
            app.click_targets
                .push((row, crate::app::ClickTarget::GitModeOption(index)));
        }
        return;
    }

    let git = &mut app.git;
    let expanded = git.commit_modal && git.confirm.is_none() && !git.picking;
    if !git.has_modal() {
        return;
    }
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage(if expanded { 5 } else { 15 }),
        Constraint::Percentage(if expanded { 90 } else { 70 }),
        Constraint::Percentage(if expanded { 5 } else { 15 }),
    ])
    .areas(frame.area());
    let [_, area, _] = Layout::horizontal([
        Constraint::Percentage(if expanded { 5 } else { 15 }),
        Constraint::Percentage(if expanded { 90 } else { 70 }),
        Constraint::Percentage(if expanded { 5 } else { 15 }),
    ])
    .areas(center);
    frame.render_widget(Clear, area);
    let branch = git
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.status.ref_name.as_deref())
        .unwrap_or("detached / unknown");
    if let Some(operation) = &git.confirm {
        let block = crate::ui::panel(" Confirm Git action ", true)
            .title_bottom(if matches!(operation, Operation::Push) {
                " Ctrl+Enter: push · Esc: cancel "
            } else {
                " Enter/Space: confirm · Esc: cancel "
            })
            .style(Style::default().bg(Color::Black).fg(Color::White));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let action = match operation {
            Operation::Commit(message) => format!("Commit ALL changed files\n\n{message}"),
            Operation::CommitPush(message) => {
                format!("Commit ALL changed files and push\n\n{message}")
            }
            Operation::Push => "Push current branch to its remote".into(),
            Operation::Switch(branch) => format!("Switch to branch {branch}"),
            _ => String::new(),
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{action}\n\nCurrent branch: {branch}\n{}",
                git.cwd.as_deref().unwrap_or_default()
            ))
            .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    if git.picking {
        let block = crate::ui::panel(" Choose branch ", true)
            .title_bottom(" ↑↓/jk: select · Enter/Space: switch · Esc: cancel ")
            .style(Style::default().bg(Color::Black).fg(Color::White));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let branches = git
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.branches.as_slice())
            .unwrap_or_default();
        let offset = git
            .branch_index
            .saturating_sub(inner.height.saturating_sub(1) as usize);
        let lines: Vec<_> = branches
            .iter()
            .enumerate()
            .skip(offset)
            .take(inner.height as usize)
            .map(|(index, name)| {
                Line::styled(
                    name.clone(),
                    if index == git.branch_index {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), inner);
        for index in offset..(offset + inner.height as usize).min(branches.len()) {
            app.click_targets.push((
                Rect::new(inner.x, inner.y + (index - offset) as u16, inner.width, 1),
                crate::app::ClickTarget::GitBranchOption(index),
            ));
        }
        return;
    }
    let generating = git
        .request
        .as_ref()
        .is_some_and(|request| matches!(request.operation, Operation::Generate(_)))
        || (git.pending.is_some() && git.notice == "Generating message…");
    let block = crate::ui::panel(
        if git.commit_push {
            " Commit & push "
        } else {
            " Commit all changes "
        },
        true,
    )
    .title_bottom(if git.editing {
        " Ctrl+Enter: submit · Esc: stop typing · Ctrl+G: generate "
    } else {
        " Ctrl+Enter: submit · Enter/i: type · g: generate · Tab: files/message · Esc: close "
    })
    .style(Style::default().bg(Color::Black).fg(Color::White));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let files_count = git
        .snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.status.working_tree.files.len());
    let [files_area, msg_area, info_area] = Layout::vertical([
        Constraint::Length(
            (files_count as u16)
                .saturating_add(2)
                .clamp(3, (inner.height / 3).max(3)),
        ),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .areas(inner);
    let files_title = format!(" Changed files ({files_count}) · all changes ");
    let files_block = crate::ui::panel(&files_title, git.commit_row == 1);
    let files_inner = files_block.inner(files_area);
    git.files_area = files_inner;
    git.files_scroll = git
        .files_scroll
        .min(files_count.saturating_sub(files_inner.height as usize));
    frame.render_widget(files_block, files_area);
    if let Some(snapshot) = &git.snapshot {
        for (row, file) in snapshot
            .status
            .working_tree
            .files
            .iter()
            .skip(git.files_scroll)
            .take(files_inner.height as usize)
            .enumerate()
        {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(format!("{} ", file.path)),
                    Span::styled(
                        format!("+{}", file.insertions),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        format!(" -{}", file.deletions),
                        Style::default().fg(Color::Red),
                    ),
                ])),
                Rect::new(
                    files_inner.x,
                    files_inner.y + row as u16,
                    files_inner.width,
                    1,
                ),
            );
        }
    }
    app.click_targets
        .push((files_area, crate::app::ClickTarget::GitFiles));
    let message_block = crate::ui::panel(
        if generating {
            " Commit message · Generating… "
        } else if git.editing {
            " Commit message (typing) "
        } else {
            " Commit message "
        },
        git.commit_row == 0,
    );
    let message_inner = message_block.inner(msg_area);
    git.message_area = message_inner;
    app.click_targets
        .push((msg_area, crate::app::ClickTarget::GitMessage));
    let (rows, (cursor_row, cursor_column)) = crate::composer::layout(
        &git.message,
        git.message.chars().count(),
        message_inner.width as usize,
    );
    let max_scroll = rows.len().saturating_sub(message_inner.height as usize);
    git.message_scroll = if git.editing && git.message_follow_end {
        max_scroll
    } else {
        git.message_scroll.min(max_scroll)
    };
    let message_block = message_block.title_bottom(Line::styled(
        format!(
            " {}–{} / {} · wheel to scroll ",
            if rows.is_empty() {
                0
            } else {
                git.message_scroll + 1
            },
            (git.message_scroll + message_inner.height as usize).min(rows.len()),
            rows.len()
        ),
        Style::default().fg(Color::Blue),
    ));
    frame.render_widget(message_block, msg_area);
    let visible = rows
        .into_iter()
        .skip(git.message_scroll)
        .take(message_inner.height as usize)
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(Paragraph::new(visible), message_inner);
    if git.editing
        && message_inner.width > 0
        && message_inner.height > 0
        && cursor_row >= git.message_scroll
        && cursor_row < git.message_scroll + message_inner.height as usize
    {
        frame.set_cursor_position((
            message_inner.x + cursor_column.min(message_inner.width as usize - 1) as u16,
            message_inner.y + (cursor_row - git.message_scroll) as u16,
        ));
    }
    let mut info = vec![Line::styled(
        format!("Branch: {branch}"),
        Style::default().fg(Color::DarkGray),
    )];
    if let Some(error) = &git.error {
        info.push(Line::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    if !git.notice.is_empty() {
        info.push(Line::styled(
            git.notice.clone(),
            Style::default().fg(Color::Cyan),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(info)).wrap(Wrap { trim: false }),
        info_area,
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn status() -> Value {
        json!({"isRepo":true,"refName":"main","hasPrimaryRemote":true,"aheadCount":1,"behindCount":0,"workingTree":{"files":[{"path":"src/main.rs","insertions":3,"deletions":1}]}})
    }
    fn ready() -> App {
        let mut app = App::default();
        app.git.cwd = Some("/repo".into());
        app.git.snapshot = Some(Snapshot {
            generated_message: Some("Add word navigation to the composer".into()),
            status: serde_json::from_value(status()).unwrap(),
            branches: vec!["main".into(), "feature".into()],
        });
        app
    }

    #[test]
    fn large_commit_message_scrolls_without_changing_text_or_actions() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = ready();
        app.git.commit_modal = true;
        app.git.message = (0..80)
            .map(|n| format!("Line {n:02}: explain a meaningful part of the change"))
            .collect::<Vec<_>>()
            .join("\n");
        let original = app.git.message.clone();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let render = |terminal: &mut Terminal<TestBackend>, app: &mut App| {
            terminal.draw(|frame| draw_modal(frame, app)).unwrap();
        };
        let visible = |terminal: &Terminal<TestBackend>| {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        render(&mut terminal, &mut app);
        assert!(app.git.message_area.height > 10);
        assert!(app.git.message_area.width > 90);
        assert!(visible(&terminal).contains("Line 00"));
        assert!(!visible(&terminal).contains("Line 79"));
        press(&mut app, KeyCode::PageDown);
        render(&mut terminal, &mut app);
        assert!(app.git.message_scroll > 0);
        let area = app.git.message_area;
        let before = app.git.message_scroll;
        crate::input::handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(app.git.message_scroll < before);
        press(&mut app, KeyCode::End);
        render(&mut terminal, &mut app);
        assert!(visible(&terminal).contains("Line 79"));
        assert_eq!(app.git.commit_row, 0);
        assert_eq!(app.git.message, original);
        assert!(app.git.request.is_none());
        press(&mut app, KeyCode::Home);
        render(&mut terminal, &mut app);
        assert!(visible(&terminal).contains("Line 00"));
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Char('!'));
        render(&mut terminal, &mut app);
        assert!(visible(&terminal).contains("change!"));
        assert!(app.git.message_scroll > 0);
        for (width, height) in [(50, 18), (20, 8), (1, 1)] {
            let mut small = Terminal::new(TestBackend::new(width, height)).unwrap();
            render(&mut small, &mut app);
        }
    }

    #[test]
    fn generating_a_message_only_refreshes_and_preserves_concurrent_edits() {
        let mut app = ready();
        app.git.message = "Previous draft".into();
        press(&mut app, KeyCode::Char('g'));
        let request = app.git.request.take().unwrap();
        assert!(matches!(request.operation, Operation::Generate(_)));
        assert!(request.payload().get("action").is_none());
        app.git.finish(
            &request,
            Ok(Snapshot {
                generated_message: Some("Add word navigation to the composer".into()),
                status: serde_json::from_value(status()).unwrap(),
                branches: vec![],
            }),
        );
        assert_eq!(app.git.message, "Add word navigation to the composer");
        assert!(app.git.confirm.is_none());
        press(&mut app, KeyCode::Char('g'));
        let request = app.git.request.take().unwrap();
        app.git.message = "My newer draft".into();
        app.git.finish(
            &request,
            Ok(Snapshot {
                generated_message: Some("Add word navigation to the composer".into()),
                status: serde_json::from_value(status()).unwrap(),
                branches: vec![],
            }),
        );
        assert_eq!(app.git.message, "My newer draft");
    }
    fn press(app: &mut App, code: KeyCode) {
        key(
            app,
            KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        );
    }
    #[test]
    fn modes_submit_only_with_ctrl_enter_and_never_auto_retry() {
        let mut app = ready();
        app.git.row = 1;
        press(&mut app, KeyCode::Enter);
        assert!(app.git.picking_mode);
        assert!(!app.git.commit_modal);
        press(&mut app, KeyCode::Down); // Commit
        press(&mut app, KeyCode::Enter);
        assert!(app.git.commit_modal);
        assert!(!app.git.commit_push);
        app.git.message = "Fix UI".into();
        press(&mut app, KeyCode::Enter);
        assert!(app.git.editing);
        assert!(app.git.request.is_none());
        let submit = |app: &mut App| {
            crate::input::handle_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL))
        };
        submit(&mut app);
        assert!(!app.git.commit_modal);
        let request = app.git.request.take().unwrap();
        assert_eq!(request.payload()["action"], "commit");
        assert_eq!(request.payload()["commitMessage"], "Fix UI");
        assert_eq!(request.payload()["cwd"], "/repo");
        // A repeated key cannot dispatch again, and failure preserves the draft.
        crate::input::handle_key(
            &mut app,
            KeyEvent::new_with_kind(
                KeyCode::Enter,
                KeyModifiers::CONTROL,
                crossterm::event::KeyEventKind::Repeat,
            ),
        );
        assert!(app.git.request.is_none());
        app.git.finish(&request, Err("connection lost".into()));
        assert_eq!(app.git.message, "Fix UI");
        assert!(app.git.request.is_none());
        app.git.select_mode(0);
        app.git.message = "Ship it".into();
        submit(&mut app);
        assert!(!app.git.commit_modal);
        let request = app.git.request.take().unwrap();
        assert_eq!(request.payload()["action"], "commit_push");
        app.git.finish(
            &request,
            Ok(Snapshot {
                generated_message: None,
                status: serde_json::from_value(status()).unwrap(),
                branches: vec!["main".into()],
            }),
        );
        assert!(!app.git.commit_modal);
        app.git.select_mode(2);
        assert!(app.git.confirm.is_some());
        press(&mut app, KeyCode::Enter);
        assert!(app.git.request.is_none());
        submit(&mut app);
        assert!(matches!(
            app.git.request.as_ref().unwrap().operation,
            Operation::Push
        ));
    }

    #[test]
    fn commit_validation_blocks_empty_no_changes_and_missing_remote() {
        let mut app = ready();
        app.git.select_mode(0);
        let submit = |app: &mut App| key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        submit(&mut app);
        assert!(app.git.error.as_deref().unwrap().contains("message"));
        app.git.message = "Preserve me".into();
        app.git.snapshot.as_mut().unwrap().status.has_primary_remote = false;
        submit(&mut app);
        assert_eq!(app.git.error.as_deref(), Some("No remote configured."));
        assert!(app.git.request.is_none());
        app.git.select_mode(1);
        app.git
            .snapshot
            .as_mut()
            .unwrap()
            .status
            .working_tree
            .files
            .clear();
        submit(&mut app);
        assert_eq!(app.git.error.as_deref(), Some("No changes to commit."));
        assert_eq!(app.git.message, "Preserve me");
        assert!(app.git.request.is_none());
    }

    #[test]
    fn uses_selected_thread_worktree_and_rejects_late_results() {
        let mut app = ready();
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        catalog.threads[0].worktree_path = Some("/worktree".into());
        app.update_catalog(catalog);
        app.move_selection(true);
        assert_eq!(workspace(&app).as_deref(), Some("/worktree"));
        app.git.queue(Operation::Refresh);
        let old = app.git.request.take().unwrap();
        app.git = GitState {
            cwd: Some("/different".into()),
            ..Default::default()
        };
        app.git.finish(&old, Err("old error".into()));
        assert!(app.git.error.is_none());
    }
}
