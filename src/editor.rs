//! T3-backed workspace file browser and editor panel.
use crate::{
    app::{App, ClickTarget, Focus},
    git::workspace,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub enum Operation {
    ListEntries(Option<String>),
    ReadFile(String),
    WriteFile { path: String, contents: String },
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
            Operation::ListEntries(directory) => match directory {
                Some(dir) => json!({ "cwd": self.cwd, "directoryPath": dir }),
                None => json!({ "cwd": self.cwd }),
            },
            Operation::ReadFile(relative_path) => {
                json!({ "cwd": self.cwd, "relativePath": relative_path })
            }
            Operation::WriteFile { path, contents } => {
                json!({ "cwd": self.cwd, "relativePath": path, "contents": contents })
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub kind: String,
    #[serde(default)]
    pub ignored: Option<bool>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == "directory"
    }

    pub fn display_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

pub enum Response {
    Entries {
        directory: Option<String>,
        entries: Vec<Entry>,
        truncated: bool,
    },
    File {
        relative_path: String,
        contents: String,
        truncated: bool,
        exists: bool,
    },
    Saved {
        relative_path: String,
    },
}

pub struct EditorState {
    pub cwd: Option<String>,
    pub directory: Option<String>,
    pub entries: Vec<Entry>,
    pub entries_truncated: bool,
    pub selected_entry: usize,
    pub tree_offset: usize,
    tree_selections: HashMap<Option<String>, String>,
    pub tree_focus: bool,
    pub open_file: Option<String>,
    pub file_truncated: bool,
    pub lines: Vec<String>,
    pub dirty: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub editing: bool,
    pub pending: Option<String>,
    pub request: Option<Request>,
    pub notice: String,
    pub error: Option<String>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            cwd: None,
            directory: None,
            entries: Vec::new(),
            entries_truncated: false,
            selected_entry: 0,
            tree_offset: 0,
            tree_selections: HashMap::new(),
            tree_focus: true,
            open_file: None,
            file_truncated: false,
            lines: Vec::new(),
            dirty: false,
            cursor_line: 0,
            cursor_col: 0,
            scroll_y: 0,
            scroll_x: 0,
            editing: false,
            pending: None,
            request: None,
            notice: String::new(),
            error: None,
        }
    }
}

impl EditorState {
    pub fn queue(&mut self, operation: Operation) {
        if self.pending.is_some() {
            return;
        }
        let Some(cwd) = &self.cwd else {
            self.error = Some("Select a project or thread with a workspace first.".into());
            return;
        };
        let id = uuid::Uuid::new_v4().to_string();
        self.pending = Some(id.clone());
        self.error = None;
        self.notice = match &operation {
            Operation::ListEntries(_) => "Loading files…".into(),
            Operation::ReadFile(path) => format!("Opening {path}…"),
            Operation::WriteFile { path, .. } => format!("Saving {path}…"),
        };
        self.request = Some(Request {
            id,
            cwd: cwd.clone(),
            operation,
        });
    }

    pub fn finish(&mut self, request: &Request, result: Result<Response, String>) {
        if self.pending.as_deref() != Some(&request.id) || self.cwd.as_deref() != Some(&request.cwd)
        {
            return;
        }
        self.pending = None;
        match result {
            Ok(Response::Entries {
                directory,
                mut entries,
                truncated,
            }) => {
                entries.retain(|entry| entry.display_name() != ".git");
                entries.sort_by(|a, b| {
                    b.is_dir()
                        .cmp(&a.is_dir())
                        .then_with(|| a.display_name().cmp(b.display_name()))
                });
                if let Some(entry) = self.entry_at(self.selected_entry) {
                    self.tree_selections
                        .insert(self.directory.clone(), entry.path.clone());
                }
                let changed_directory = self.directory != directory;
                let returning_from = self.directory.as_ref().filter(|current| {
                    current
                        .rsplit_once('/')
                        .map(|(parent, _)| parent.to_owned())
                        == directory
                });
                let preferred = returning_from.or_else(|| self.tree_selections.get(&directory));
                let selected =
                    preferred.and_then(|path| entries.iter().position(|entry| &entry.path == path));
                self.selected_entry = selected
                    .map(|index| index + usize::from(directory.is_some()))
                    .unwrap_or(0);
                if changed_directory {
                    self.tree_offset = 0;
                }
                self.directory = directory;
                self.entries = entries;
                self.entries_truncated = truncated;
                self.notice.clear();
            }
            Ok(Response::File {
                relative_path,
                contents,
                truncated,
                exists,
            }) => {
                if !exists {
                    self.error = Some(format!("File not found: {relative_path}"));
                    self.notice.clear();
                    return;
                }
                self.open_file = Some(relative_path);
                self.file_truncated = truncated;
                self.lines = if contents.is_empty() {
                    vec![String::new()]
                } else {
                    contents.split('\n').map(str::to_owned).collect()
                };
                self.dirty = false;
                self.cursor_line = 0;
                self.cursor_col = 0;
                self.scroll_y = 0;
                self.scroll_x = 0;
                self.editing = false;
                self.tree_focus = false;
                self.notice = if truncated {
                    "File truncated by server limit.".into()
                } else {
                    String::new()
                };
            }
            Ok(Response::Saved { relative_path }) => {
                self.dirty = false;
                self.notice = format!("Saved {relative_path}.");
            }
            Err(error) => {
                self.error = Some(error);
                self.notice.clear();
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        usize::from(self.directory.is_some()) + self.entries.len()
    }

    fn entry_at(&self, index: usize) -> Option<&Entry> {
        index
            .checked_sub(usize::from(self.directory.is_some()))
            .and_then(|index| self.entries.get(index))
    }

    pub fn move_tree(&mut self, down: bool, amount: usize) {
        self.selected_entry = if down {
            self.selected_entry
                .saturating_add(amount)
                .min(self.entry_count().saturating_sub(1))
        } else {
            self.selected_entry.saturating_sub(amount)
        };
    }

    pub fn activate_entry(&mut self, index: usize) {
        if self.directory.is_some() {
            if index == 0 {
                self.go_up();
                return;
            }
            let Some(entry) = self.entries.get(index - 1).cloned() else {
                return;
            };
            if entry.is_dir() {
                self.queue(Operation::ListEntries(Some(entry.path)));
            } else {
                self.queue(Operation::ReadFile(entry.path));
            }
        } else {
            let Some(entry) = self.entries.get(index).cloned() else {
                return;
            };
            if entry.is_dir() {
                self.queue(Operation::ListEntries(Some(entry.path)));
            } else {
                self.queue(Operation::ReadFile(entry.path));
            }
        }
    }

    pub fn go_up(&mut self) {
        let Some(current) = &self.directory else {
            return;
        };
        let parent = current
            .rsplit_once('/')
            .map(|(head, _)| head.to_owned())
            .filter(|head| !head.is_empty());
        self.queue(Operation::ListEntries(parent));
    }

    fn save(&mut self) {
        let Some(path) = self.open_file.clone() else {
            return;
        };
        if self.file_truncated {
            self.error = Some("Cannot save a truncated file.".into());
            return;
        }
        let contents = self.lines.join("\n");
        self.queue(Operation::WriteFile { path, contents });
    }

    fn clamp_col(&mut self) {
        let len = self
            .lines
            .get(self.cursor_line)
            .map_or(0, |line| line.chars().count());
        self.cursor_col = self.cursor_col.min(len);
    }

    fn byte_index(line: &str, char_col: usize) -> usize {
        line.char_indices()
            .nth(char_col)
            .map_or(line.len(), |(idx, _)| idx)
    }

    fn insert_char(&mut self, ch: char) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        let line = &mut self.lines[self.cursor_line];
        let byte = Self::byte_index(line, self.cursor_col);
        line.insert(byte, ch);
        self.cursor_col += 1;
        self.dirty = true;
    }

    fn insert_newline(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        let line = &mut self.lines[self.cursor_line];
        let byte = Self::byte_index(line, self.cursor_col);
        let tail = line.split_off(byte);
        self.cursor_line += 1;
        self.lines.insert(self.cursor_line, tail);
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_line];
            let end = Self::byte_index(line, self.cursor_col);
            let start = Self::byte_index(line, self.cursor_col - 1);
            line.replace_range(start..end, "");
            self.cursor_col -= 1;
            self.dirty = true;
        } else if self.cursor_line > 0 {
            let tail = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let prev_len = self.lines[self.cursor_line].chars().count();
            self.lines[self.cursor_line].push_str(&tail);
            self.cursor_col = prev_len;
            self.dirty = true;
        }
    }

    fn delete(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        let len = self.lines[self.cursor_line].chars().count();
        if self.cursor_col < len {
            let line = &mut self.lines[self.cursor_line];
            let start = Self::byte_index(line, self.cursor_col);
            let end = Self::byte_index(line, self.cursor_col + 1);
            line.replace_range(start..end, "");
            self.dirty = true;
        } else if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
            self.dirty = true;
        }
    }
}

pub fn sync(app: &mut App) {
    let cwd = workspace(app);
    if app.editor.cwd != cwd {
        app.editor = EditorState {
            cwd,
            ..Default::default()
        };
    }
    if app.workspace_visible
        && app.workspace_tab == 0
        && app.authenticated
        && app.editor.pending.is_none()
        && app.editor.cwd.is_some()
        && app.editor.entries.is_empty()
        && app.editor.error.is_none()
    {
        app.editor
            .queue(Operation::ListEntries(app.editor.directory.clone()));
    }
}

pub fn key(app: &mut App, key: KeyEvent) -> bool {
    let editor = &mut app.editor;
    if editor.editing {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            editor.save();
            return true;
        }
        match key.code {
            KeyCode::Esc => editor.editing = false,
            KeyCode::Enter => editor.insert_newline(),
            KeyCode::Backspace => editor.backspace(),
            KeyCode::Delete => editor.delete(),
            KeyCode::Up if editor.cursor_line > 0 => {
                editor.cursor_line -= 1;
                editor.clamp_col();
            }
            KeyCode::Down if editor.cursor_line + 1 < editor.lines.len() => {
                editor.cursor_line += 1;
                editor.clamp_col();
            }
            KeyCode::Left if editor.cursor_col > 0 => editor.cursor_col -= 1,
            KeyCode::Right => {
                let len = editor
                    .lines
                    .get(editor.cursor_line)
                    .map_or(0, |line| line.chars().count());
                if editor.cursor_col < len {
                    editor.cursor_col += 1;
                }
            }
            KeyCode::Home => editor.cursor_col = 0,
            KeyCode::End => {
                editor.cursor_col = editor
                    .lines
                    .get(editor.cursor_line)
                    .map_or(0, |line| line.chars().count());
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                editor.insert_char(ch);
            }
            _ => (),
        }
        return true;
    }

    if editor.tree_focus {
        let count = editor.entry_count();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if editor.selected_entry == 0 {
                    return false;
                }
                editor.selected_entry -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if editor.selected_entry + 1 < count {
                    editor.selected_entry += 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => {
                editor.activate_entry(editor.selected_entry);
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace
                if editor.directory.is_some() =>
            {
                editor.go_up();
            }
            KeyCode::Tab | KeyCode::BackTab if editor.open_file.is_some() => {
                editor.tree_focus = false;
            }
            KeyCode::Home => editor.selected_entry = 0,
            KeyCode::End => editor.selected_entry = count.saturating_sub(1),
            KeyCode::PageDown => editor.move_tree(true, 10),
            KeyCode::PageUp => editor.move_tree(false, 10),
            KeyCode::Char('r') => {
                editor.queue(Operation::ListEntries(editor.directory.clone()));
            }
            _ => return false,
        }
        return true;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        editor.save();
        return true;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => editor.tree_focus = true,
        KeyCode::Enter | KeyCode::Char('i') if editor.open_file.is_some() => {
            editor.editing = true;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if editor.cursor_line == 0 {
                return false;
            }
            editor.cursor_line -= 1;
            editor.clamp_col();
        }
        KeyCode::Down | KeyCode::Char('j') if editor.cursor_line + 1 < editor.lines.len() => {
            editor.cursor_line += 1;
            editor.clamp_col();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if editor.cursor_col == 0 {
                editor.tree_focus = true;
            } else {
                editor.cursor_col -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let len = editor
                .lines
                .get(editor.cursor_line)
                .map_or(0, |line| line.chars().count());
            if editor.cursor_col < len {
                editor.cursor_col += 1;
            }
        }
        KeyCode::Home => editor.cursor_col = 0,
        KeyCode::End => {
            editor.cursor_col = editor
                .lines
                .get(editor.cursor_line)
                .map_or(0, |line| line.chars().count());
        }
        KeyCode::PageUp => {
            editor.cursor_line = editor.cursor_line.saturating_sub(15);
            editor.clamp_col();
        }
        KeyCode::PageDown if !editor.lines.is_empty() => {
            editor.cursor_line = (editor.cursor_line + 15).min(editor.lines.len() - 1);
            editor.clamp_col();
        }
        KeyCode::Char('r') if editor.open_file.is_some() => {
            if let Some(path) = editor.open_file.clone() {
                editor.queue(Operation::ReadFile(path));
            }
        }
        _ => return false,
    }
    true
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if app.editor.cwd.is_none() {
        frame.render_widget(
            Paragraph::new("Select a project or thread to browse its workspace.")
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    if !app.authenticated {
        frame.render_widget(
            Paragraph::new("Sign in / pair with T3 (p) to browse and edit workspace files.")
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    if app.editor.tree_focus || app.editor.open_file.is_none() {
        draw_tree(frame, app, area);
    } else if area.width >= 56 {
        let [code_area, tree_area] =
            Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
                .areas(area);
        draw_tree(frame, app, tree_area);
        draw_code(frame, app, code_area);
    } else if app.editor.tree_focus {
        draw_tree(frame, app, area);
    } else {
        draw_code(frame, app, area);
    }
}

/// Truncate in the middle so long filenames retain their extension.
fn compact_name(name: &str, width: usize) -> String {
    if name.width() <= width {
        return name.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let graphemes: Vec<_> = name.graphemes(true).collect();
    let suffix_budget = (width - 1) / 3;
    let mut suffix = String::new();
    for grapheme in graphemes.iter().rev() {
        if suffix.width() + grapheme.width() > suffix_budget {
            break;
        }
        suffix.insert_str(0, grapheme);
    }
    let mut prefix = String::new();
    for grapheme in graphemes {
        if prefix.width() + grapheme.width() + suffix.width() + 1 > width {
            break;
        }
        prefix.push_str(grapheme);
    }
    format!("{prefix}…{suffix}")
}

fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Workspace && app.editor.tree_focus;
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let outer = Block::default()
        .borders(if app.editor.tree_focus || app.editor.open_file.is_none() {
            Borders::RIGHT
        } else {
            Borders::LEFT
        })
        .border_style(Style::default().fg(border_color));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.height < 3 || inner.width == 0 {
        return;
    }
    let [header, list, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
    let root_name = app
        .editor
        .cwd
        .as_deref()
        .and_then(|cwd| cwd.rsplit('/').find(|s| !s.is_empty()))
        .unwrap_or("Workspace");
    let path = app.editor.directory.as_deref().unwrap_or("./");
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!(
                    " Files · {}",
                    compact_name(root_name, inner.width.saturating_sub(10) as usize)
                ),
                Style::default()
                    .fg(if focused { Color::Cyan } else { Color::White })
                    .bold(),
            ),
            Line::styled(
                format!(
                    " {}",
                    compact_name(path, inner.width.saturating_sub(2) as usize)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(border_color)),
        ),
        header,
    );

    let folders = app
        .editor
        .entries
        .iter()
        .filter(|entry| entry.is_dir())
        .count();
    let status = app
        .editor
        .error
        .clone()
        .or_else(|| {
            app.editor
                .pending
                .as_ref()
                .map(|_| app.editor.notice.clone())
        })
        .unwrap_or_else(|| {
            if app.editor.entries.is_empty() {
                "Empty folder".into()
            } else {
                format!(
                    "{folders} folders · {} files{}",
                    app.editor.entries.len() - folders,
                    if app.editor.entries_truncated {
                        " · partial"
                    } else {
                        ""
                    }
                )
            }
        });
    frame.render_widget(
        Paragraph::new(format!(" {status}")).style(Style::default().fg(
            if app.editor.error.is_some() {
                Color::Yellow
            } else {
                Color::DarkGray
            },
        )),
        footer,
    );

    let count = app.editor.entry_count();
    let visible = list.height as usize;
    if visible == 0 || count == 0 {
        return;
    }
    app.editor.selected_entry = app.editor.selected_entry.min(count - 1);
    let selected = app.editor.selected_entry;
    let offset = app.editor.tree_offset.min(count.saturating_sub(visible));
    let offset = if selected < offset {
        selected
    } else if selected >= offset + visible {
        selected + 1 - visible
    } else {
        offset
    };
    app.editor.tree_offset = offset;
    let changed_files = app
        .git
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.status.working_tree.files.as_slice())
        .unwrap_or(&[]);
    for index in offset..(offset + visible).min(count) {
        let entry = app.editor.entry_at(index);
        let is_parent = entry.is_none();
        let is_dir = entry.is_some_and(Entry::is_dir);
        let open =
            entry.is_some_and(|entry| app.editor.open_file.as_deref() == Some(entry.path.as_str()));
        let changed = entry.is_some_and(|entry| is_entry_changed(entry, changed_files))
            || entry.is_some_and(|entry| {
                open && app.editor.dirty
                    && app.editor.open_file.as_deref() == Some(entry.path.as_str())
            });
        let selected = index == selected;
        let row_style = if selected {
            Style::default().bg(if focused {
                Color::Rgb(30, 53, 65)
            } else {
                Color::Rgb(32, 35, 40)
            })
        } else {
            Style::default()
        };
        let color = if is_parent {
            Color::DarkGray
        } else if is_dir || open {
            Color::Cyan
        } else if entry.is_some_and(|entry| entry.ignored == Some(true)) {
            Color::DarkGray
        } else {
            Color::Gray
        };
        let name = if is_parent {
            "..".into()
        } else {
            let entry = entry.unwrap();
            format!("{}{}", entry.display_name(), if is_dir { "/" } else { "" })
        };
        let name = compact_name(&name, list.width.saturating_sub(7) as usize);
        let padding = list
            .width
            .saturating_sub(7)
            .saturating_sub(name.width() as u16) as usize;
        let line = Line::from(vec![
            Span::styled(
                if selected { "› " } else { "  " },
                row_style.fg(if focused { Color::Cyan } else { Color::Gray }),
            ),
            Span::styled(
                if is_parent {
                    "↑ "
                } else if is_dir {
                    "▸ "
                } else {
                    "· "
                },
                row_style.fg(color),
            ),
            Span::styled(
                format!("{name}{}", " ".repeat(padding)),
                if is_dir || open {
                    row_style.fg(color).bold()
                } else {
                    row_style.fg(color)
                },
            ),
            Span::styled(if open { "●" } else { " " }, row_style.fg(Color::Cyan)),
            Span::styled(
                if changed { " M" } else { "  " },
                row_style.fg(Color::Yellow),
            ),
        ]);
        let row = Rect::new(list.x, list.y + (index - offset) as u16, list.width, 1);
        frame.render_widget(Paragraph::new(line).style(row_style), row);
        app.click_targets
            .push((row, ClickTarget::EditorEntry(index)));
    }
}

fn is_entry_changed(entry: &Entry, changed_files: &[crate::git::File]) -> bool {
    if entry.is_dir() {
        let prefix = format!("{}/", entry.path);
        changed_files
            .iter()
            .any(|file| file.path == entry.path || file.path.starts_with(&prefix))
    } else {
        changed_files.iter().any(|file| file.path == entry.path)
    }
}

fn draw_code(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height < 3 || area.width == 0 {
        return;
    }
    let [header_area, body_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let code_focused = !app.editor.tree_focus && app.focus == Focus::Workspace;
    let header_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(if code_focused {
            Color::Blue
        } else {
            Color::DarkGray
        }));
    let header_inner = header_block.inner(header_area);
    frame.render_widget(header_block, header_area);

    let editor = &mut app.editor;
    let header_line = match &editor.open_file {
        Some(path) => {
            let mut spans = vec![Span::raw(" ")];
            if let Some((dir, name)) = path.rsplit_once('/') {
                spans.push(Span::styled(
                    format!("{dir}/"),
                    Style::default().fg(Color::DarkGray),
                ));
                spans.push(Span::styled(
                    name.to_owned(),
                    Style::default()
                        .fg(if code_focused {
                            Color::Cyan
                        } else {
                            Color::White
                        })
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    path.clone(),
                    Style::default()
                        .fg(if code_focused {
                            Color::Cyan
                        } else {
                            Color::White
                        })
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if editor.dirty {
                spans.push(Span::styled(
                    " ● modified",
                    Style::default().fg(Color::Yellow),
                ));
            }
            if editor.editing {
                spans.push(Span::styled(
                    " [EDITING]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Line::from(spans)
        }
        None => Line::styled(" No file open", Style::default().fg(Color::DarkGray)),
    };
    frame.render_widget(Paragraph::new(header_line), header_inner);

    if editor.open_file.is_none() {
        frame.render_widget(
            Paragraph::new("Select a file from the list to view or edit.")
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false }),
            body_area,
        );
    } else {
        let visible_rows = body_area.height as usize;
        if visible_rows > 0 {
            if editor.cursor_line < editor.scroll_y {
                editor.scroll_y = editor.cursor_line;
            } else if editor.cursor_line >= editor.scroll_y + visible_rows {
                editor.scroll_y = editor.cursor_line + 1 - visible_rows;
            }
        }
        let gutter_width = editor.lines.len().max(1).to_string().len() + 1;
        let code_width = (body_area.width as usize).saturating_sub(gutter_width + 1);
        if code_width > 0 {
            if editor.cursor_col < editor.scroll_x {
                editor.scroll_x = editor.cursor_col;
            } else if editor.cursor_col >= editor.scroll_x + code_width {
                editor.scroll_x = editor.cursor_col + 1 - code_width;
            }
        }
        let mut rendered = Vec::new();
        for (line_idx, line_text) in editor
            .lines
            .iter()
            .enumerate()
            .skip(editor.scroll_y)
            .take(visible_rows)
        {
            let num = format!("{:>width$} ", line_idx + 1, width = gutter_width - 1);
            let slice: String = line_text
                .chars()
                .skip(editor.scroll_x)
                .take(code_width.max(1))
                .collect();
            let mut spans = vec![Span::styled(num, Style::default().fg(Color::DarkGray))];
            if line_idx == editor.cursor_line && !editor.tree_focus && app.focus == Focus::Workspace
            {
                let rel_col = editor.cursor_col.saturating_sub(editor.scroll_x);
                let chars: Vec<char> = slice.chars().collect();
                let before: String = chars.iter().take(rel_col).collect();
                let cursor_ch = chars.get(rel_col).copied().unwrap_or(' ');
                let after: String = chars.iter().skip(rel_col + 1).collect();
                let cursor_style = if editor.editing {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                };
                spans.push(Span::raw(before));
                spans.push(Span::styled(cursor_ch.to_string(), cursor_style));
                spans.push(Span::raw(after));
            } else {
                spans.push(Span::raw(slice));
            }
            rendered.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(Text::from(rendered)), body_area);
    }

    let status_text = editor
        .error
        .as_deref()
        .map(|err| (err.to_owned(), Color::Yellow))
        .filter(|(s, _)| !s.is_empty())
        .or_else(|| (!editor.notice.is_empty()).then(|| (editor.notice.clone(), Color::Cyan)))
        .unwrap_or_else(|| {
            (
                format!(
                    "Ln {}, Col {}",
                    editor.cursor_line + 1,
                    editor.cursor_col + 1
                ),
                Color::DarkGray,
            )
        });
    frame.render_widget(
        Paragraph::new(status_text.0).style(Style::default().fg(status_text.1)),
        status_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_read_and_write_roundtrip_and_rejects_stale_cwd() {
        let mut state = EditorState {
            cwd: Some("/repo".into()),
            ..Default::default()
        };
        state.queue(Operation::ListEntries(None));
        let req = state.request.take().unwrap();
        assert_eq!(req.payload()["cwd"], "/repo");
        state.finish(
            &req,
            Ok(Response::Entries {
                directory: None,
                entries: vec![
                    Entry {
                        path: "Cargo.toml".into(),
                        kind: "file".into(),
                        ignored: None,
                    },
                    Entry {
                        path: "src".into(),
                        kind: "directory".into(),
                        ignored: None,
                    },
                    Entry {
                        path: ".git".into(),
                        kind: "directory".into(),
                        ignored: Some(true),
                    },
                ],
                truncated: false,
            }),
        );
        assert_eq!(state.entries.len(), 2);
        assert!(state.entries[0].is_dir());
        assert_eq!(state.entries[0].path, "src");
        assert_eq!(state.entries[1].path, "Cargo.toml");

        state.activate_entry(1);
        let read_req = state.request.take().unwrap();
        assert_eq!(read_req.payload()["relativePath"], "Cargo.toml");
        state.finish(
            &read_req,
            Ok(Response::File {
                relative_path: "Cargo.toml".into(),
                contents: "[package]\nname = \"tui3\"".into(),
                truncated: false,
                exists: true,
            }),
        );
        assert_eq!(state.open_file.as_deref(), Some("Cargo.toml"));
        assert_eq!(state.lines.len(), 2);
        assert!(!state.tree_focus);

        state.insert_char('#');
        assert!(state.dirty);
        state.save();
        let write_req = state.request.take().unwrap();
        assert_eq!(write_req.payload()["relativePath"], "Cargo.toml");
        assert_eq!(
            write_req.payload()["contents"],
            "#[package]\nname = \"tui3\""
        );
        state.finish(
            &write_req,
            Ok(Response::Saved {
                relative_path: "Cargo.toml".into(),
            }),
        );
        assert!(!state.dirty);

        // Stale cwd results must be ignored.
        state.queue(Operation::ListEntries(None));
        let stale = state.request.take().unwrap();
        state.cwd = Some("/other".into());
        state.finish(&stale, Err("old error".into()));
        assert!(state.error.is_none());
    }

    #[test]
    fn changed_entries_match_files_and_parent_directories() {
        let changed = vec![crate::git::File {
            path: "src/main.rs".into(),
            insertions: 2,
            deletions: 1,
        }];
        let src_dir = Entry {
            path: "src".into(),
            kind: "directory".into(),
            ignored: None,
        };
        let src2_dir = Entry {
            path: "src2".into(),
            kind: "directory".into(),
            ignored: None,
        };
        let main_file = Entry {
            path: "src/main.rs".into(),
            kind: "file".into(),
            ignored: None,
        };
        let other_file = Entry {
            path: "Cargo.toml".into(),
            kind: "file".into(),
            ignored: None,
        };
        assert!(is_entry_changed(&src_dir, &changed));
        assert!(!is_entry_changed(&src2_dir, &changed));
        assert!(is_entry_changed(&main_file, &changed));
        assert!(!is_entry_changed(&other_file, &changed));
    }

    #[test]
    fn tree_navigation_uses_arrows_and_hjkl_even_with_an_open_file() {
        for (right, left, down, up) in [
            (KeyCode::Right, KeyCode::Left, KeyCode::Down, KeyCode::Up),
            (
                KeyCode::Char('l'),
                KeyCode::Char('h'),
                KeyCode::Char('j'),
                KeyCode::Char('k'),
            ),
            (
                KeyCode::Enter,
                KeyCode::Backspace,
                KeyCode::Down,
                KeyCode::Up,
            ),
            (
                KeyCode::Char(' '),
                KeyCode::Backspace,
                KeyCode::Down,
                KeyCode::Up,
            ),
        ] {
            let mut app = App::default();
            app.editor.cwd = Some("/repo".into());
            app.editor.open_file = Some("existing.rs".into());
            app.editor.entries = vec![
                Entry {
                    path: "src".into(),
                    kind: "directory".into(),
                    ignored: None,
                },
                Entry {
                    path: "Cargo.toml".into(),
                    kind: "file".into(),
                    ignored: None,
                },
            ];
            let press = |app: &mut App, code| key(app, KeyEvent::new(code, KeyModifiers::NONE));
            assert!(press(&mut app, down));
            assert_eq!(app.editor.selected_entry, 1);
            assert!(press(&mut app, up));
            assert_eq!(app.editor.selected_entry, 0);
            assert!(press(&mut app, right));
            let request = app.editor.request.take().unwrap();
            assert!(
                matches!(&request.operation, Operation::ListEntries(Some(path)) if path == "src")
            );
            assert!(app.editor.tree_focus);
            app.editor.finish(
                &request,
                Ok(Response::Entries {
                    directory: Some("src".into()),
                    entries: vec![Entry {
                        path: "src/main.rs".into(),
                        kind: "file".into(),
                        ignored: None,
                    }],
                    truncated: false,
                }),
            );
            assert!(app.editor.pending.is_none());
            assert!(press(&mut app, left));
            let request = app.editor.request.take().unwrap();
            assert!(matches!(request.operation, Operation::ListEntries(None)));
            app.editor.finish(
                &request,
                Ok(Response::Entries {
                    directory: None,
                    entries: vec![Entry {
                        path: "Cargo.toml".into(),
                        kind: "file".into(),
                        ignored: None,
                    }],
                    truncated: false,
                }),
            );
            assert!(press(&mut app, right));
            let request = app.editor.request.take().unwrap();
            assert!(
                matches!(&request.operation, Operation::ReadFile(path) if path == "Cargo.toml")
            );
            app.editor.finish(
                &request,
                Ok(Response::File {
                    relative_path: "Cargo.toml".into(),
                    contents: "[package]".into(),
                    truncated: false,
                    exists: true,
                }),
            );
            assert!(!app.editor.tree_focus);
        }
    }

    #[test]
    fn enter_and_space_open_selected_files_in_code_pane() {
        for code in [KeyCode::Enter, KeyCode::Char(' ')] {
            for nested in [false, true] {
                let mut app = App {
                    focus: Focus::Workspace,
                    workspace_tab: 0,
                    ..Default::default()
                };
                app.editor.cwd = Some("/repo".into());
                app.editor.directory = nested.then(|| "src".into());
                let path = if nested { "src/main.rs" } else { "main.rs" };
                app.editor.entries = vec![Entry {
                    path: path.into(),
                    kind: "file".into(),
                    ignored: None,
                }];
                app.editor.selected_entry = usize::from(nested);
                crate::input::handle_key(&mut app, KeyEvent::new(code, KeyModifiers::NONE));
                let request = app.editor.request.take().unwrap();
                assert!(matches!(&request.operation, Operation::ReadFile(file) if file == path));
                app.editor.finish(
                    &request,
                    Ok(Response::File {
                        relative_path: path.into(),
                        contents: "fn main() {}".into(),
                        truncated: false,
                        exists: true,
                    }),
                );
                assert!(!app.editor.tree_focus);
                assert!(!app.editor.editing);
                assert_eq!(app.editor.open_file.as_deref(), Some(path));
            }
        }
    }

    #[test]
    fn refresh_and_parent_navigation_restore_selection_by_path() {
        let entry = |path: &str, kind: &str| Entry {
            path: path.into(),
            kind: kind.into(),
            ignored: None,
        };
        let mut state = EditorState {
            cwd: Some("/repo".into()),
            ..Default::default()
        };
        state.entries = vec![entry("docs", "directory"), entry("src", "directory")];
        state.selected_entry = 1;
        state.activate_entry(1);
        let request = state.request.take().unwrap();
        state.finish(
            &request,
            Ok(Response::Entries {
                directory: Some("src".into()),
                entries: vec![entry("src/main.rs", "file")],
                truncated: false,
            }),
        );
        state.selected_entry = 1;
        state.queue(Operation::ListEntries(Some("src".into())));
        let request = state.request.take().unwrap();
        state.finish(
            &request,
            Ok(Response::Entries {
                directory: Some("src".into()),
                entries: vec![entry("src/lib.rs", "file"), entry("src/main.rs", "file")],
                truncated: false,
            }),
        );
        assert_eq!(
            state.entry_at(state.selected_entry).unwrap().path,
            "src/main.rs"
        );
        state.go_up();
        let request = state.request.take().unwrap();
        state.finish(
            &request,
            Ok(Response::Entries {
                directory: None,
                entries: vec![entry("src", "directory"), entry("docs", "directory")],
                truncated: false,
            }),
        );
        assert_eq!(state.entry_at(state.selected_entry).unwrap().path, "src");
        state.activate_entry(state.selected_entry);
        let request = state.request.take().unwrap();
        state.finish(
            &request,
            Ok(Response::Entries {
                directory: Some("src".into()),
                entries: vec![entry("src/lib.rs", "file"), entry("src/main.rs", "file")],
                truncated: false,
            }),
        );
        assert_eq!(
            state.entry_at(state.selected_entry).unwrap().path,
            "src/main.rs"
        );
    }

    #[test]
    fn explorer_scrolls_at_edges_and_keeps_mouse_targets_aligned() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App {
            focus: Focus::Workspace,
            ..Default::default()
        };
        app.editor.cwd = Some("/repo".into());
        app.editor.entries = (0..40)
            .map(|index| Entry {
                path: format!("file-{index:02}.rs"),
                kind: "file".into(),
                ignored: None,
            })
            .collect();
        let mut terminal = Terminal::new(TestBackend::new(32, 12)).unwrap();
        let render = |terminal: &mut Terminal<TestBackend>, app: &mut App| {
            app.click_targets.clear();
            terminal
                .draw(|frame| draw_tree(frame, app, frame.area()))
                .unwrap();
        };
        render(&mut terminal, &mut app);
        app.editor.selected_entry = 2;
        render(&mut terminal, &mut app);
        assert_eq!(app.editor.tree_offset, 0);
        let (row, _) = app
            .click_targets
            .iter()
            .find(|(_, target)| matches!(target, ClickTarget::EditorEntry(2)))
            .unwrap();
        let row = *row;
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(row.x, row.y)].bg,
            buffer[(row.right() - 1, row.y)].bg
        );
        crate::input::handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: row.x + 2,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.editor.selected_entry, 5);
        key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        render(&mut terminal, &mut app);
        assert!(app.editor.tree_offset > 0);
        let (row, _) = app
            .click_targets
            .iter()
            .find(|(_, target)| matches!(target, ClickTarget::EditorEntry(39)))
            .unwrap();
        let text = (row.x..row.right())
            .map(|x| terminal.backend().buffer()[(x, row.y)].symbol())
            .collect::<String>();
        assert!(text.contains("file-39.rs"));
        app.editor.entries[39].path = "very-long-unicode-文件名-with-extension.rs".into();
        app.editor.open_file = Some(app.editor.entries[39].path.clone());
        app.editor.dirty = true;
        render(&mut terminal, &mut app);
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains('…'));
        assert!(text.contains("● M"));
    }

    #[test]
    fn explorer_fills_editor_until_file_opens_and_tab_restores_browser() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App {
            authenticated: true,
            focus: Focus::Workspace,
            ..Default::default()
        };
        app.editor.cwd = Some("/repo".into());
        app.editor.entries = vec![Entry {
            path: "main.rs".into(),
            kind: "file".into(),
            ignored: None,
        }];
        let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
        let render = |terminal: &mut Terminal<TestBackend>, app: &mut App| {
            app.click_targets.clear();
            terminal
                .draw(|frame| draw(frame, app, frame.area()))
                .unwrap();
        };
        let entry_width = |app: &App| {
            app.click_targets
                .iter()
                .find_map(|(area, target)| {
                    matches!(target, ClickTarget::EditorEntry(0)).then_some(area.width)
                })
                .unwrap()
        };
        render(&mut terminal, &mut app);
        assert_eq!(entry_width(&app), 89);
        key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let request = app.editor.request.take().unwrap();
        app.editor.finish(
            &request,
            Ok(Response::File {
                relative_path: "main.rs".into(),
                contents: "fn main() {}".into(),
                truncated: false,
                exists: true,
            }),
        );
        render(&mut terminal, &mut app);
        assert!(entry_width(&app) < 45);
        let explorer = app
            .click_targets
            .iter()
            .find_map(|(area, target)| {
                matches!(target, ClickTarget::EditorEntry(0)).then_some(*area)
            })
            .unwrap();
        assert!(
            explorer.x > 45,
            "Explorer should be to the right of the code"
        );
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("fn main()"));
        key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        render(&mut terminal, &mut app);
        assert_eq!(entry_width(&app), 89);
    }

    #[test]
    fn tab_toggles_between_file_tree_and_code_view() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            ..Default::default()
        };
        app.editor.open_file = Some("Cargo.toml".into());
        app.editor.lines = vec!["[package]".into()];
        app.editor.tree_focus = true;

        assert!(key(
            &mut app,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
        ));
        assert!(!app.editor.tree_focus);

        assert!(key(
            &mut app,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
        ));
        assert!(app.editor.tree_focus);
    }
}
