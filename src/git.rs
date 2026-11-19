//! Small T3-backed branch / commit / push panel.
use crate::{
    app::{App, Focus},
    catalog::Selection,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Paragraph, Wrap},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub enum Operation {
    Generate(String),
    Refresh,
    Switch(String),
    Commit(String),
    Push,
}
#[derive(Clone)]
pub struct Request {
    pub id: String,
    pub cwd: String,
    pub operation: Operation,
}
impl Request {
    pub fn payload(&self) -> Value {
        match &self.operation {
            Operation::Switch(branch) => json!({"cwd":self.cwd,"refName":branch}),
            Operation::Commit(message) => {
                json!({"actionId":self.id,"cwd":self.cwd,"action":"commit","commitMessage":message,"featureBranch":false})
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

#[derive(Default)]
pub struct GitState {
    pub cwd: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub pending: Option<String>,
    pub request: Option<Request>,
    pub notice: String,
    pub error: Option<String>,
    pub row: usize,
    pub message: String,
    pub editing: bool,
    pub picking: bool,
    pub branch_index: usize,
    pub confirm: Option<Operation>,
    pub refreshed: Option<std::time::Instant>,
}
impl GitState {
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
                    } else {
                        self.error = Some("No changes to describe.".into());
                    }
                }
                if matches!(&request.operation, Operation::Commit(text) if text == self.message.trim())
                {
                    self.message.clear();
                }
                self.notice = match request.operation {
                    Operation::Refresh => String::new(),
                    Operation::Generate(_) if self.error.is_some() => String::new(),
                    Operation::Generate(_) => {
                        "Generated an AI message from the diff. Review before committing.".into()
                    }
                    Operation::Switch(_) => "Branch switched.".into(),
                    Operation::Commit(_) => "Commit finished.".into(),
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
    if app.git.cwd != cwd {
        app.git = GitState {
            cwd,
            ..Default::default()
        };
    }
    if app.workspace_tab == 3
        && app.authenticated
        && app.git.pending.is_none()
        && !app.git.editing
        && !app.git.picking
        && app.git.confirm.is_none()
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
    if key.code == KeyCode::Char('p')
        && !app.authenticated
        && !app.git.editing
        && !app.git.picking
        && app.git.confirm.is_none()
    {
        if !app.pairing_in_progress {
            app.pairing_requested = true;
        }
        return true;
    }
    if key.code == KeyCode::Enter && key.kind == crossterm::event::KeyEventKind::Repeat {
        return true;
    }
    let git = &mut app.git;
    if git.editing {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => git.editing = false,
            KeyCode::Backspace => {
                use unicode_segmentation::UnicodeSegmentation;
                if let Some((byte, _)) = git.message.grapheme_indices(true).next_back() {
                    git.message.truncate(byte);
                }
            }
            KeyCode::Char(ch) => git.message.push(ch),
            _ => (),
        }
        return true;
    }
    if git.pending.is_some() {
        return false;
    }
    if let Some(operation) = git.confirm.take() {
        if key.code == KeyCode::Enter {
            git.queue(operation);
        } else if key.code != KeyCode::Esc {
            git.confirm = Some(operation);
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
            KeyCode::Enter => {
                if let Some(branch) = git
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.branches.get(git.branch_index))
                {
                    git.confirm = Some(Operation::Switch(branch.clone()));
                }
                git.picking = false;
            }
            _ => (),
        }
        return true;
    }
    match key.code {
        KeyCode::Char('g') => git.queue(Operation::Generate(git.message.clone())),
        KeyCode::Char('r') => git.queue(Operation::Refresh),
        KeyCode::Up | KeyCode::Char('k') if git.row > 0 => git.row = git.row.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') if git.row < 3 => git.row += 1,
        KeyCode::Enter => {
            if !git
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.is_repo)
            {
                return true;
            }
            match git.row {
                0 => {
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
                }
                1 => git.editing = true,
                2 if git.message.trim().is_empty() => {
                    git.error = Some("Enter a commit message first.".into())
                }
                2 if git
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.status.working_tree.files.is_empty()) =>
                {
                    git.error = Some("No changes to commit.".into())
                }
                2 => git.confirm = Some(Operation::Commit(git.message.trim().into())),
                3 if !git
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.status.has_primary_remote) =>
                {
                    git.error = Some("No remote configured.".into())
                }
                3 => git.confirm = Some(Operation::Push),
                _ => (),
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
    if let Some(operation) = &git.confirm {
        let action = match operation {
            Operation::Commit(message) => format!("Commit ALL changed files\n\n{message}"),
            Operation::Push => "Push current branch to its remote".into(),
            Operation::Switch(branch) => format!("Switch to branch {branch}"),
            _ => String::new(),
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{action}\n\nCurrent branch: {branch}\n{}\n\nEnter: confirm · Esc: cancel",
                git.cwd.as_deref().unwrap_or_default()
            ))
            .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    if git.picking {
        let branches = git
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.branches.as_slice())
            .unwrap_or_default();
        let offset = git
            .branch_index
            .saturating_sub(inner.height.saturating_sub(2) as usize);
        let lines: Vec<_> = std::iter::once(Line::from("Choose branch · Enter / Esc"))
            .chain(
                branches
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(inner.height.saturating_sub(1) as usize)
                    .map(|(index, name)| {
                        Line::styled(
                            name.clone(),
                            if index == git.branch_index {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Gray)
                            },
                        )
                    }),
            )
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), inner);
        return;
    }
    let [controls, details] =
        Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).areas(inner);
    let rows = Layout::vertical([Constraint::Length(3); 4]).split(controls);
    let labels = [
        format!("Branch: {branch}"),
        format!(
            "Message{}: {}",
            if git.editing { " (typing)" } else { "" },
            git.message
        ),
        "Commit all changes".into(),
        "Push".into(),
    ];
    let generating = git
        .request
        .as_ref()
        .is_some_and(|request| matches!(request.operation, Operation::Generate(_)))
        || (git.pending.is_some() && git.notice == "Generating message…");
    for (index, rect) in rows.iter().enumerate() {
        if index == 1 {
            let message = *rect;
            let message_block = crate::ui::panel(
                if generating {
                    " Commit message · Generating… "
                } else if git.editing {
                    " Commit message (typing) "
                } else {
                    " Commit message "
                },
                app.focus == Focus::Workspace && git.row == 1,
            );
            let message_block = if generating {
                message_block.title_bottom(" Please wait · g generates a draft ")
            } else {
                message_block
            };
            crate::composer::draw(
                frame,
                if git.editing {
                    &git.message
                } else {
                    git.message.lines().next().unwrap_or_default()
                },
                git.message.chars().count(),
                git.editing,
                message_block,
                message,
            );
            app.click_targets
                .push((message, crate::app::ClickTarget::GitControl(1)));
        } else {
            frame.render_widget(
                Paragraph::new(labels[index].as_str()).block(crate::ui::panel(
                    "",
                    app.focus == Focus::Workspace && git.row == index,
                )),
                *rect,
            );
        }
        if index != 1 {
            app.click_targets
                .push((*rect, crate::app::ClickTarget::GitControl(index)));
        }
    }
    let mut lines = vec![Line::styled(
        git.cwd
            .clone()
            .unwrap_or("Select a project or thread.".into()),
        Style::default().fg(Color::DarkGray),
    )];
    if let Some(snapshot) = &git.snapshot {
        if !snapshot.status.is_repo {
            lines.push(Line::from("Not a Git repository."));
        } else {
            lines.push(Line::from(format!(
                "↑{} ahead · ↓{} behind · {} changed files",
                snapshot.status.ahead_count,
                snapshot.status.behind_count,
                snapshot.status.working_tree.files.len()
            )));
            for file in snapshot
                .status
                .working_tree
                .files
                .iter()
                .take(details.height.saturating_sub(5) as usize)
            {
                lines.push(Line::from(format!(
                    "{} +{} -{}",
                    file.path, file.insertions, file.deletions
                )));
            }
        }
    }
    if let Some(error) = &git.error {
        lines.push(Line::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines.push(Line::styled(
        git.notice.clone(),
        Style::default().fg(Color::Cyan),
    ));
    lines.push(Line::from(
        "↑↓/jk: select · Enter: open · g: generate · r: refresh",
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        details,
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
    fn commit_and_push_require_deliberate_confirmation_and_never_auto_retry() {
        let mut app = ready();
        app.git.row = 2;
        app.git.message = "Fix UI".into();
        press(&mut app, KeyCode::Enter);
        assert!(app.git.request.is_none());
        assert!(app.git.confirm.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(app.git.request.is_none());
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        let request = app.git.request.take().unwrap();
        assert_eq!(request.payload()["action"], "commit");
        assert_eq!(request.payload()["commitMessage"], "Fix UI");
        assert_eq!(request.payload()["cwd"], "/repo");
        app.git.finish(&request, Err("connection lost".into()));
        assert_eq!(app.git.message, "Fix UI");
        assert!(app.git.request.is_none());
        app.git.row = 3;
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.git.request.as_ref().unwrap().operation,
            Operation::Push
        ));
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
