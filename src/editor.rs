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
    WriteFile {
        path: String,
        contents: String,
    },
    CreateFile {
        path: String,
        directory: Option<String>,
    },
    CreateFolder {
        path: String,
        directory: Option<String>,
    },
    DeleteEntry {
        path: String,
        is_dir: bool,
        directory: Option<String>,
    },
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
            Operation::CreateFile { path, .. } => {
                json!({ "cwd": self.cwd, "relativePath": path, "contents": "" })
            }
            Operation::CreateFolder { path, .. } | Operation::DeleteEntry { path, .. } => {
                json!({ "cwd": self.cwd, "relativePath": path })
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
    Mutated {
        directory: Option<String>,
        entries: Vec<Entry>,
        truncated: bool,
        select_path: Option<String>,
        deleted_path: Option<String>,
        notice: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreePrompt {
    NewFile(String),
    NewFolder(String),
    ConfirmDelete {
        path: String,
        is_dir: bool,
        name: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Normal,
    Insert,
    Visual,
}

pub struct EditorState {
    pub cwd: Option<String>,
    pub directory: Option<String>,
    pub entries: Vec<Entry>,
    pub entries_truncated: bool,
    pub selected_entry: usize,
    pub tree_offset: usize,
    tree_selections: HashMap<Option<String>, String>,
    pub tree_searching: bool,
    pub tree_search: String,
    pub tree_prompt: Option<TreePrompt>,
    pub tree_focus: bool,
    pub open_file: Option<String>,
    pub file_truncated: bool,
    pub lines: Vec<String>,
    pub dirty: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub visible_rows: usize,
    pub editing: bool,
    pub mode: EditorMode,
    pub selection_anchor: Option<(usize, usize)>,
    pub yank_register: String,
    pub pending_key: Option<char>,
    undo_stack: Vec<(Vec<String>, usize, usize)>,
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
            tree_searching: false,
            tree_search: String::new(),
            tree_prompt: None,
            tree_focus: true,
            open_file: None,
            file_truncated: false,
            lines: Vec::new(),
            dirty: false,
            cursor_line: 0,
            cursor_col: 0,
            scroll_y: 0,
            scroll_x: 0,
            visible_rows: 0,
            editing: false,
            mode: EditorMode::Normal,
            selection_anchor: None,
            yank_register: String::new(),
            pending_key: None,
            undo_stack: Vec::new(),
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
            Operation::CreateFile { path, .. } => format!("Creating {path}…"),
            Operation::CreateFolder { path, .. } => format!("Creating {path}/…"),
            Operation::DeleteEntry { path, .. } => format!("Deleting {path}…"),
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
                self.mode = EditorMode::Normal;
                self.selection_anchor = None;
                self.pending_key = None;
                self.undo_stack.clear();
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
            Ok(Response::Mutated {
                directory,
                mut entries,
                truncated,
                select_path,
                deleted_path,
                notice,
            }) => {
                entries.retain(|entry| entry.display_name() != ".git");
                entries.sort_by(|a, b| {
                    b.is_dir()
                        .cmp(&a.is_dir())
                        .then_with(|| a.display_name().cmp(b.display_name()))
                });
                if let Some(deleted) = &deleted_path
                    && let Some(open) = &self.open_file
                    && (open == deleted || open.starts_with(&format!("{deleted}/")))
                {
                    self.open_file = None;
                    self.lines.clear();
                    self.dirty = false;
                    self.tree_focus = true;
                }
                let offset = usize::from(directory.is_some());
                if let Some(target) = &select_path
                    && let Some(pos) = entries.iter().position(|entry| &entry.path == target)
                {
                    self.selected_entry = pos + offset;
                } else {
                    let count = offset + entries.len();
                    self.selected_entry = self.selected_entry.min(count.saturating_sub(1));
                }
                self.directory = directory;
                self.entries = entries;
                self.entries_truncated = truncated;
                self.notice = notice;
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
            } else if crate::imgview::is_image_path(&entry.path) {
                self.open_image_file(entry.path);
            } else {
                self.queue(Operation::ReadFile(entry.path));
            }
        } else {
            let Some(entry) = self.entries.get(index).cloned() else {
                return;
            };
            if entry.is_dir() {
                self.queue(Operation::ListEntries(Some(entry.path)));
            } else if crate::imgview::is_image_path(&entry.path) {
                self.open_image_file(entry.path);
            } else {
                self.queue(Operation::ReadFile(entry.path));
            }
        }
    }

    fn open_image_file(&mut self, path: String) {
        self.open_file = Some(path);
        self.file_truncated = false;
        self.lines.clear();
        self.dirty = false;
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_y = 0;
        self.scroll_x = 0;
        self.editing = false;
        self.mode = EditorMode::Normal;
        self.selection_anchor = None;
        self.pending_key = None;
        self.undo_stack.clear();
        self.tree_focus = false;
        self.error = None;
        self.notice.clear();
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

    fn line_len(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, |l| l.chars().count())
    }

    fn push_undo(&mut self) {
        if self.undo_stack.len() >= 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack
            .push((self.lines.clone(), self.cursor_line, self.cursor_col));
    }

    fn undo(&mut self) -> bool {
        if let Some((lines, line, col)) = self.undo_stack.pop() {
            self.lines = lines;
            self.cursor_line = line.min(self.lines.len().saturating_sub(1));
            self.cursor_col = col;
            self.clamp_col();
            self.selection_anchor = None;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    fn move_to(&mut self, line: usize, col: usize, select: bool) {
        if select {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some((self.cursor_line, self.cursor_col));
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor_line = line.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(self.line_len(self.cursor_line));
    }

    pub fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.selection_anchor?;
        let cursor = (self.cursor_line, self.cursor_col);
        if anchor <= cursor {
            Some((anchor, cursor))
        } else {
            Some((cursor, anchor))
        }
    }

    fn selected_text(&self) -> Option<String> {
        let ((sl, sc), (el, ec)) = self.selection_range()?;
        if (sl, sc) == (el, ec) {
            return None;
        }
        if sl == el {
            let line = self.lines.get(sl)?;
            let start = Self::byte_index(line, sc);
            let end = Self::byte_index(line, ec);
            return Some(line[start..end].to_owned());
        }
        let mut out = String::new();
        for l in sl..=el {
            let Some(line) = self.lines.get(l) else {
                continue;
            };
            if l == sl {
                let start = Self::byte_index(line, sc);
                out.push_str(&line[start..]);
            } else if l == el {
                let end = Self::byte_index(line, ec);
                out.push('\n');
                out.push_str(&line[..end]);
            } else {
                out.push('\n');
                out.push_str(line);
            }
        }
        Some(out)
    }

    fn delete_selection(&mut self) -> bool {
        let Some(((sl, sc), (el, ec))) = self.selection_range() else {
            return false;
        };
        self.selection_anchor = None;
        if (sl, sc) == (el, ec) || self.lines.is_empty() {
            return false;
        }
        self.push_undo();
        if sl == el {
            let line = &mut self.lines[sl];
            let start = Self::byte_index(line, sc);
            let end = Self::byte_index(line, ec);
            line.replace_range(start..end, "");
        } else {
            let head_end = Self::byte_index(&self.lines[sl], sc);
            let head = self.lines[sl][..head_end].to_owned();
            let tail_start = Self::byte_index(&self.lines[el], ec);
            let tail = self.lines[el][tail_start..].to_owned();
            self.lines.drain(sl..=el);
            self.lines.insert(sl, format!("{head}{tail}"));
        }
        self.cursor_line = sl;
        self.cursor_col = sc;
        self.dirty = true;
        true
    }

    fn insert_str(&mut self, text: &str) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        for (idx, part) in text.split('\n').enumerate() {
            if idx > 0 {
                self.insert_newline();
            }
            if !part.is_empty() {
                let line = &mut self.lines[self.cursor_line];
                let byte = Self::byte_index(line, self.cursor_col);
                line.insert_str(byte, part);
                self.cursor_col += part.chars().count();
                self.dirty = true;
            }
        }
    }

    fn delete_word_backward(&mut self) {
        if self.cursor_col == 0 {
            self.backspace();
            return;
        }
        let line = &self.lines[self.cursor_line];
        let chars: Vec<char> = line.chars().collect();
        let mut target = self.cursor_col;
        while target > 0 && chars[target - 1].is_whitespace() {
            target -= 1;
        }
        while target > 0 && !chars[target - 1].is_whitespace() {
            target -= 1;
        }
        let start = Self::byte_index(line, target);
        let end = Self::byte_index(line, self.cursor_col);
        self.lines[self.cursor_line].replace_range(start..end, "");
        self.cursor_col = target;
        self.dirty = true;
    }

    fn move_word_forward(&mut self, select: bool) {
        if self.lines.is_empty() {
            return;
        }
        let mut line = self.cursor_line;
        let mut col = self.cursor_col;
        let chars: Vec<char> = self.lines[line].chars().collect();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        if col < chars.len() {
            if is_word(chars[col]) {
                while col < chars.len() && is_word(chars[col]) {
                    col += 1;
                }
            } else if !chars[col].is_whitespace() {
                while col < chars.len() && !is_word(chars[col]) && !chars[col].is_whitespace() {
                    col += 1;
                }
            }
            while col < chars.len() && chars[col].is_whitespace() {
                col += 1;
            }
        }
        if col >= chars.len() && line + 1 < self.lines.len() {
            line += 1;
            let next_chars: Vec<char> = self.lines[line].chars().collect();
            col = 0;
            while col < next_chars.len() && next_chars[col].is_whitespace() {
                col += 1;
            }
        }
        self.move_to(line, col, select);
    }

    fn move_word_backward(&mut self, select: bool) {
        if self.lines.is_empty() {
            return;
        }
        let mut line = self.cursor_line;
        let mut col = self.cursor_col;
        if col == 0 {
            if line > 0 {
                line -= 1;
                col = self.line_len(line);
            }
        } else {
            let chars: Vec<char> = self.lines[line].chars().collect();
            let is_word = |c: char| c.is_alphanumeric() || c == '_';
            col -= 1;
            while col > 0 && chars[col].is_whitespace() {
                col -= 1;
            }
            if is_word(chars[col]) {
                while col > 0 && is_word(chars[col - 1]) {
                    col -= 1;
                }
            } else if !chars[col].is_whitespace() {
                while col > 0 && !is_word(chars[col - 1]) && !chars[col - 1].is_whitespace() {
                    col -= 1;
                }
            }
        }
        self.move_to(line, col, select);
    }

    fn move_word_end(&mut self, select: bool) {
        if self.lines.is_empty() {
            return;
        }
        let line = self.cursor_line;
        let chars: Vec<char> = self.lines[line].chars().collect();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut col = self.cursor_col;
        if col + 1 < chars.len() {
            col += 1;
            while col < chars.len() && chars[col].is_whitespace() {
                col += 1;
            }
            if col < chars.len() {
                if is_word(chars[col]) {
                    while col + 1 < chars.len() && is_word(chars[col + 1]) {
                        col += 1;
                    }
                } else {
                    while col + 1 < chars.len()
                        && !is_word(chars[col + 1])
                        && !chars[col + 1].is_whitespace()
                    {
                        col += 1;
                    }
                }
            }
        } else if line + 1 < self.lines.len() {
            self.move_to(line + 1, 0, select);
            return;
        }
        self.move_to(line, col, select);
    }

    pub fn tree_search_matches(&self) -> Vec<usize> {
        let query = self.tree_search.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let offset = usize::from(self.directory.is_some());
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.display_name().to_ascii_lowercase().contains(&query))
            .map(|(idx, _)| idx + offset)
            .collect()
    }

    fn select_first_tree_match(&mut self) {
        if let Some(first) = self.tree_search_matches().first().copied() {
            self.selected_entry = first;
        }
    }

    fn cycle_tree_match(&mut self, forward: bool) {
        let matches = self.tree_search_matches();
        if matches.is_empty() {
            return;
        }
        let pos = matches.iter().position(|&idx| idx == self.selected_entry);
        let next = match (pos, forward) {
            (Some(i), true) => (i + 1) % matches.len(),
            (Some(i), false) => (i + matches.len() - 1) % matches.len(),
            (None, true) => 0,
            (None, false) => matches.len() - 1,
        };
        self.selected_entry = matches[next];
    }

    pub fn tree_has_input(&self) -> bool {
        self.tree_searching || self.tree_prompt.is_some()
    }

    fn join_current_dir(&self, raw_name: &str) -> Result<String, String> {
        let name = raw_name.trim().trim_matches('/');
        if name.is_empty() || name.contains('\0') || name.contains('\\') {
            return Err("Enter a valid name.".into());
        }
        for component in name.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err("Path cannot contain empty, '.' or '..' segments.".into());
            }
        }
        Ok(match &self.directory {
            Some(dir) if !dir.is_empty() => format!("{dir}/{name}"),
            _ => name.to_owned(),
        })
    }

    fn delete_current_line(&mut self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        self.push_undo();
        let removed = self.lines.remove(self.cursor_line);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_line = self.cursor_line.min(self.lines.len() - 1);
        self.clamp_col();
        self.dirty = true;
        format!("{removed}\n")
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
    if app.editor.tree_focus {
        return key_tree(&mut app.editor, key);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s' | 'S'))
    {
        app.editor.save();
        return true;
    }
    match app.typing_style {
        crate::preferences::TypingStyle::Regular => key_regular(app, key),
        crate::preferences::TypingStyle::Vim => key_vim(app, key),
        crate::preferences::TypingStyle::Helix => key_helix(app, key),
    }
}

fn key_tree(editor: &mut EditorState, key: KeyEvent) -> bool {
    if let Some(prompt) = editor.tree_prompt.take() {
        match prompt {
            TreePrompt::NewFile(mut name) => match key.code {
                KeyCode::Esc => (),
                KeyCode::Enter => match editor.join_current_dir(&name) {
                    Ok(path) => editor.queue(Operation::CreateFile {
                        path,
                        directory: editor.directory.clone(),
                    }),
                    Err(err) => {
                        editor.error = Some(err);
                        editor.tree_prompt = Some(TreePrompt::NewFile(name));
                    }
                },
                KeyCode::Backspace => {
                    name.pop();
                    editor.tree_prompt = Some(TreePrompt::NewFile(name));
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    name.push(ch);
                    editor.tree_prompt = Some(TreePrompt::NewFile(name));
                }
                _ => editor.tree_prompt = Some(TreePrompt::NewFile(name)),
            },
            TreePrompt::NewFolder(mut name) => match key.code {
                KeyCode::Esc => (),
                KeyCode::Enter => match editor.join_current_dir(&name) {
                    Ok(path) => editor.queue(Operation::CreateFolder {
                        path,
                        directory: editor.directory.clone(),
                    }),
                    Err(err) => {
                        editor.error = Some(err);
                        editor.tree_prompt = Some(TreePrompt::NewFolder(name));
                    }
                },
                KeyCode::Backspace => {
                    name.pop();
                    editor.tree_prompt = Some(TreePrompt::NewFolder(name));
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    name.push(ch);
                    editor.tree_prompt = Some(TreePrompt::NewFolder(name));
                }
                _ => editor.tree_prompt = Some(TreePrompt::NewFolder(name)),
            },
            TreePrompt::ConfirmDelete { path, is_dir, name } => match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                    editor.queue(Operation::DeleteEntry {
                        path,
                        is_dir,
                        directory: editor.directory.clone(),
                    });
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => (),
                _ => editor.tree_prompt = Some(TreePrompt::ConfirmDelete { path, is_dir, name }),
            },
        }
        return true;
    }
    if editor.tree_searching {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('n') {
            editor.cycle_tree_match(true);
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            editor.cycle_tree_match(false);
            return true;
        }
        match key.code {
            KeyCode::Esc => {
                editor.tree_searching = false;
                editor.tree_search.clear();
            }
            KeyCode::Enter => {
                let has_match =
                    editor.tree_search.trim().is_empty() || !editor.tree_search_matches().is_empty();
                editor.tree_searching = false;
                editor.tree_search.clear();
                if has_match {
                    editor.activate_entry(editor.selected_entry);
                }
            }
            KeyCode::Down => editor.cycle_tree_match(true),
            KeyCode::Up => editor.cycle_tree_match(false),
            KeyCode::Backspace => {
                editor.tree_search.pop();
                editor.select_first_tree_match();
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                editor.tree_search.push(ch);
                editor.select_first_tree_match();
            }
            _ => (),
        }
        return true;
    }
    let count = editor.entry_count();
    match key.code {
        KeyCode::Char('/') => {
            editor.tree_searching = true;
            editor.tree_search.clear();
        }
        KeyCode::Char('n' | 'a') => {
            editor.error = None;
            editor.tree_prompt = Some(TreePrompt::NewFile(String::new()));
        }
        KeyCode::Char('N' | 'A') => {
            editor.error = None;
            editor.tree_prompt = Some(TreePrompt::NewFolder(String::new()));
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Some(entry) = editor.entry_at(editor.selected_entry).cloned() {
                editor.error = None;
                let name = entry.display_name().to_owned();
                editor.tree_prompt = Some(TreePrompt::ConfirmDelete {
                    path: entry.path,
                    is_dir: entry.kind == "directory",
                    name,
                });
            }
        }
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
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace if editor.directory.is_some() => {
            editor.go_up();
        }
        KeyCode::Tab | KeyCode::BackTab if editor.open_file.is_some() => {
            editor.tree_focus = false;
            editor.editing = editor.mode == EditorMode::Insert;
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
    true
}

fn copy_editor_selection(app: &mut App, fallback_line: bool) -> bool {
    let text = app.editor.selected_text().or_else(|| {
        fallback_line
            .then(|| {
                app.editor
                    .lines
                    .get(app.editor.cursor_line)
                    .map(|l| format!("{l}\n"))
            })
            .flatten()
    });
    let Some(text) = text else {
        return false;
    };
    app.editor.yank_register = text.clone();
    app.copy_requested = Some(text);
    true
}

fn paste_text(editor: &mut EditorState, after: bool) {
    let text = editor.yank_register.clone();
    if text.is_empty() {
        return;
    }
    editor.push_undo();
    editor.delete_selection();
    if text.ends_with('\n') && editor.mode != EditorMode::Insert {
        let target_line = if after {
            (editor.cursor_line + 1).min(editor.lines.len())
        } else {
            editor.cursor_line
        };
        let inserted: Vec<String> = text
            .trim_end_matches('\n')
            .split('\n')
            .map(str::to_owned)
            .collect();
        for (i, line) in inserted.into_iter().enumerate() {
            editor.lines.insert(target_line + i, line);
        }
        editor.cursor_line = target_line;
        editor.cursor_col = 0;
        editor.dirty = true;
    } else {
        if after && editor.cursor_col < editor.line_len(editor.cursor_line) {
            editor.cursor_col += 1;
        }
        editor.insert_str(&text);
    }
}

fn key_insert_common(editor: &mut EditorState, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            editor.push_undo();
            editor.delete_selection();
            editor.insert_newline();
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.push_undo();
            if !editor.delete_selection() {
                editor.delete_word_backward();
            }
        }
        KeyCode::Backspace => {
            editor.push_undo();
            if !editor.delete_selection() {
                editor.backspace();
            }
        }
        KeyCode::Delete => {
            editor.push_undo();
            if !editor.delete_selection() {
                editor.delete();
            }
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            editor.push_undo();
            editor.delete_selection();
            editor.insert_char(ch);
        }
        _ => (),
    }
}

fn key_regular(app: &mut App, key: KeyEvent) -> bool {
    let editor = &mut app.editor;
    editor.editing = true;
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if ctrl {
        match key.code {
            KeyCode::Char('a') => {
                let last_line = editor.lines.len().saturating_sub(1);
                let last_col = editor.line_len(last_line);
                editor.selection_anchor = Some((0, 0));
                editor.cursor_line = last_line;
                editor.cursor_col = last_col;
                return true;
            }
            KeyCode::Char('c') => {
                copy_editor_selection(app, true);
                return true;
            }
            KeyCode::Char('x') => {
                if copy_editor_selection(app, false) {
                    app.editor.delete_selection();
                } else if !app.editor.lines.is_empty() {
                    let removed = app.editor.delete_current_line();
                    app.editor.yank_register = removed.clone();
                    app.copy_requested = Some(removed);
                }
                return true;
            }
            KeyCode::Char('v') => {
                paste_text(editor, false);
                return true;
            }
            KeyCode::Char('z') => {
                editor.undo();
                return true;
            }
            KeyCode::Left => {
                editor.move_word_backward(shift);
                return true;
            }
            KeyCode::Right => {
                editor.move_word_forward(shift);
                return true;
            }
            KeyCode::Home => {
                editor.move_to(0, 0, shift);
                return true;
            }
            KeyCode::End => {
                let last_line = editor.lines.len().saturating_sub(1);
                editor.move_to(last_line, editor.line_len(last_line), shift);
                return true;
            }
            _ => (),
        }
    }

    match key.code {
        KeyCode::Esc => {
            if editor.selection_anchor.is_some() {
                editor.selection_anchor = None;
            } else {
                editor.editing = false;
                editor.tree_focus = true;
            }
        }
        KeyCode::Tab | KeyCode::BackTab => {
            editor.editing = false;
            editor.tree_focus = true;
        }
        KeyCode::Up => {
            let line = editor.cursor_line.saturating_sub(1);
            editor.move_to(line, editor.cursor_col, shift);
        }
        KeyCode::Down => {
            let line = (editor.cursor_line + 1).min(editor.lines.len().saturating_sub(1));
            editor.move_to(line, editor.cursor_col, shift);
        }
        KeyCode::Left => {
            if editor.cursor_col > 0 {
                editor.move_to(editor.cursor_line, editor.cursor_col - 1, shift);
            } else if editor.cursor_line > 0 {
                let prev = editor.cursor_line - 1;
                editor.move_to(prev, editor.line_len(prev), shift);
            } else if !shift {
                editor.selection_anchor = None;
            }
        }
        KeyCode::Right => {
            let len = editor.line_len(editor.cursor_line);
            if editor.cursor_col < len {
                editor.move_to(editor.cursor_line, editor.cursor_col + 1, shift);
            } else if editor.cursor_line + 1 < editor.lines.len() {
                editor.move_to(editor.cursor_line + 1, 0, shift);
            } else if !shift {
                editor.selection_anchor = None;
            }
        }
        KeyCode::Home => editor.move_to(editor.cursor_line, 0, shift),
        KeyCode::End => editor.move_to(
            editor.cursor_line,
            editor.line_len(editor.cursor_line),
            shift,
        ),
        KeyCode::PageUp => editor.move_to(
            editor.cursor_line.saturating_sub(15),
            editor.cursor_col,
            shift,
        ),
        KeyCode::PageDown => {
            let line = (editor.cursor_line + 15).min(editor.lines.len().saturating_sub(1));
            editor.move_to(line, editor.cursor_col, shift);
        }
        _ => key_insert_common(editor, key),
    }
    true
}

fn handle_insert_mode_key(editor: &mut EditorState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            editor.mode = EditorMode::Normal;
            editor.editing = false;
            editor.selection_anchor = None;
        }
        KeyCode::Up => editor.move_to(
            editor.cursor_line.saturating_sub(1),
            editor.cursor_col,
            false,
        ),
        KeyCode::Down => {
            let line = (editor.cursor_line + 1).min(editor.lines.len().saturating_sub(1));
            editor.move_to(line, editor.cursor_col, false);
        }
        KeyCode::Left => editor.move_to(
            editor.cursor_line,
            editor.cursor_col.saturating_sub(1),
            false,
        ),
        KeyCode::Right => {
            let len = editor.line_len(editor.cursor_line);
            editor.move_to(editor.cursor_line, (editor.cursor_col + 1).min(len), false);
        }
        KeyCode::Home => editor.move_to(editor.cursor_line, 0, false),
        KeyCode::End => editor.move_to(
            editor.cursor_line,
            editor.line_len(editor.cursor_line),
            false,
        ),
        _ => key_insert_common(editor, key),
    }
}

fn enter_insert(
    editor: &mut EditorState,
    append: bool,
    line_end: bool,
    line_start: bool,
    open_line: Option<bool>,
) {
    editor.selection_anchor = None;
    editor.pending_key = None;
    if let Some(below) = open_line {
        editor.push_undo();
        let target = if below {
            (editor.cursor_line + 1).min(editor.lines.len())
        } else {
            editor.cursor_line
        };
        editor.lines.insert(target, String::new());
        editor.cursor_line = target;
        editor.cursor_col = 0;
        editor.dirty = true;
    } else if line_end {
        editor.cursor_col = editor.line_len(editor.cursor_line);
    } else if line_start {
        editor.cursor_col = 0;
    } else if append && editor.cursor_col < editor.line_len(editor.cursor_line) {
        editor.cursor_col += 1;
    }
    editor.mode = EditorMode::Insert;
    editor.editing = true;
}

fn key_vim(app: &mut App, key: KeyEvent) -> bool {
    if app.editor.mode == EditorMode::Insert {
        handle_insert_mode_key(&mut app.editor, key);
        return true;
    }

    let visual = app.editor.mode == EditorMode::Visual;
    if let Some(pending) = app.editor.pending_key.take() {
        match (pending, key.code) {
            ('g', KeyCode::Char('g')) => app.editor.move_to(0, 0, visual),
            ('d', KeyCode::Char('d')) => {
                let removed = app.editor.delete_current_line();
                app.editor.yank_register = removed.clone();
                app.copy_requested = Some(removed);
            }
            ('d', KeyCode::Char('w')) => {
                app.editor.selection_anchor = Some((app.editor.cursor_line, app.editor.cursor_col));
                app.editor.move_word_forward(true);
                if copy_editor_selection(app, false) {
                    app.editor.delete_selection();
                }
            }
            ('y', KeyCode::Char('y')) => {
                if let Some(line) = app.editor.lines.get(app.editor.cursor_line) {
                    let text = format!("{line}\n");
                    app.editor.yank_register = text.clone();
                    app.copy_requested = Some(text);
                }
            }
            ('y', KeyCode::Char('w')) => {
                let start = (app.editor.cursor_line, app.editor.cursor_col);
                app.editor.selection_anchor = Some(start);
                app.editor.move_word_forward(true);
                copy_editor_selection(app, false);
                app.editor.selection_anchor = None;
                app.editor.move_to(start.0, start.1, false);
            }
            _ => (),
        }
        return true;
    }

    match key.code {
        KeyCode::Esc => {
            if visual || app.editor.selection_anchor.is_some() {
                app.editor.mode = EditorMode::Normal;
                app.editor.selection_anchor = None;
            } else {
                app.editor.tree_focus = true;
            }
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.editor.mode = EditorMode::Normal;
            app.editor.selection_anchor = None;
            app.editor.tree_focus = true;
        }
        KeyCode::Char('v') => {
            if visual {
                app.editor.mode = EditorMode::Normal;
                app.editor.selection_anchor = None;
            } else {
                app.editor.mode = EditorMode::Visual;
                app.editor.selection_anchor = Some((app.editor.cursor_line, app.editor.cursor_col));
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.editor.move_to(
                app.editor.cursor_line,
                app.editor.cursor_col.saturating_sub(1),
                visual,
            );
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let line = (app.editor.cursor_line + 1).min(app.editor.lines.len().saturating_sub(1));
            app.editor.move_to(line, app.editor.cursor_col, visual);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.editor.move_to(
                app.editor.cursor_line.saturating_sub(1),
                app.editor.cursor_col,
                visual,
            );
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let len = app.editor.line_len(app.editor.cursor_line);
            app.editor.move_to(
                app.editor.cursor_line,
                (app.editor.cursor_col + 1).min(len),
                visual,
            );
        }
        KeyCode::Char('0') | KeyCode::Home => app.editor.move_to(app.editor.cursor_line, 0, visual),
        KeyCode::Char('$') | KeyCode::End => {
            let len = app.editor.line_len(app.editor.cursor_line);
            app.editor.move_to(app.editor.cursor_line, len, visual);
        }
        KeyCode::Char('^') => {
            let col = app
                .editor
                .lines
                .get(app.editor.cursor_line)
                .and_then(|l| l.chars().position(|c| !c.is_whitespace()))
                .unwrap_or(0);
            app.editor.move_to(app.editor.cursor_line, col, visual);
        }
        KeyCode::Char('w') => app.editor.move_word_forward(visual),
        KeyCode::Char('b') => app.editor.move_word_backward(visual),
        KeyCode::Char('e') => app.editor.move_word_end(visual),
        KeyCode::Char('G') => {
            let last = app.editor.lines.len().saturating_sub(1);
            app.editor.move_to(last, 0, visual);
        }
        KeyCode::Char('g') => app.editor.pending_key = Some('g'),
        KeyCode::PageUp => {
            app.editor.move_to(
                app.editor.cursor_line.saturating_sub(15),
                app.editor.cursor_col,
                visual,
            );
        }
        KeyCode::PageDown => {
            let line = (app.editor.cursor_line + 15).min(app.editor.lines.len().saturating_sub(1));
            app.editor.move_to(line, app.editor.cursor_col, visual);
        }
        KeyCode::Char('i') if !visual => enter_insert(&mut app.editor, false, false, false, None),
        KeyCode::Char('a') if !visual => enter_insert(&mut app.editor, true, false, false, None),
        KeyCode::Char('I') if !visual => enter_insert(&mut app.editor, false, false, true, None),
        KeyCode::Char('A') if !visual => enter_insert(&mut app.editor, false, true, false, None),
        KeyCode::Char('o') if !visual => {
            enter_insert(&mut app.editor, false, false, false, Some(true))
        }
        KeyCode::Char('O') if !visual => {
            enter_insert(&mut app.editor, false, false, false, Some(false))
        }
        KeyCode::Char('x') => {
            if visual {
                if copy_editor_selection(app, false) {
                    app.editor.delete_selection();
                }
                app.editor.mode = EditorMode::Normal;
            } else if app.editor.cursor_col < app.editor.line_len(app.editor.cursor_line) {
                app.editor.push_undo();
                let line = &app.editor.lines[app.editor.cursor_line];
                let s = EditorState::byte_index(line, app.editor.cursor_col);
                let e = EditorState::byte_index(line, app.editor.cursor_col + 1);
                let removed = line[s..e].to_owned();
                app.editor.yank_register = removed.clone();
                app.copy_requested = Some(removed);
                app.editor.delete();
            }
        }
        KeyCode::Char('d') => {
            if visual {
                if copy_editor_selection(app, false) {
                    app.editor.delete_selection();
                }
                app.editor.mode = EditorMode::Normal;
            } else {
                app.editor.pending_key = Some('d');
            }
        }
        KeyCode::Char('c') if visual => {
            if copy_editor_selection(app, false) {
                app.editor.delete_selection();
            }
            enter_insert(&mut app.editor, false, false, false, None);
        }
        KeyCode::Char('y') => {
            if visual {
                copy_editor_selection(app, false);
                app.editor.mode = EditorMode::Normal;
                app.editor.selection_anchor = None;
            } else {
                app.editor.pending_key = Some('y');
            }
        }
        KeyCode::Char('p') if !visual => paste_text(&mut app.editor, true),
        KeyCode::Char('P') if !visual => paste_text(&mut app.editor, false),
        KeyCode::Char('u') if !visual => {
            app.editor.undo();
        }
        _ => (),
    }
    true
}

fn key_helix(app: &mut App, key: KeyEvent) -> bool {
    if app.editor.mode == EditorMode::Insert {
        handle_insert_mode_key(&mut app.editor, key);
        return true;
    }

    let extend = app.editor.mode == EditorMode::Visual;
    if let Some(pending) = app.editor.pending_key.take() {
        if pending == 'g' {
            let last_line = app.editor.lines.len().saturating_sub(1);
            match key.code {
                KeyCode::Char('g') => app.editor.move_to(0, 0, extend),
                KeyCode::Char('e') => {
                    app.editor
                        .move_to(last_line, app.editor.line_len(last_line), extend);
                }
                KeyCode::Char('h') => app.editor.move_to(app.editor.cursor_line, 0, extend),
                KeyCode::Char('l') => {
                    let len = app.editor.line_len(app.editor.cursor_line);
                    app.editor.move_to(app.editor.cursor_line, len, extend);
                }
                KeyCode::Char('s') => {
                    let col = app
                        .editor
                        .lines
                        .get(app.editor.cursor_line)
                        .and_then(|l| l.chars().position(|c| !c.is_whitespace()))
                        .unwrap_or(0);
                    app.editor.move_to(app.editor.cursor_line, col, extend);
                }
                KeyCode::Char('t') => {
                    let target = app.editor.scroll_y.min(last_line);
                    app.editor.move_to(target, app.editor.cursor_col, extend);
                }
                KeyCode::Char('c') => {
                    let target = (app.editor.scroll_y + app.editor.visible_rows / 2).min(last_line);
                    app.editor.move_to(target, app.editor.cursor_col, extend);
                }
                KeyCode::Char('b') => {
                    let offset = app.editor.visible_rows.saturating_sub(1);
                    let target = (app.editor.scroll_y + offset).min(last_line);
                    app.editor.move_to(target, app.editor.cursor_col, extend);
                }
                KeyCode::Esc | _ => (),
            }
        }
        return true;
    }

    match key.code {
        KeyCode::Esc => {
            if extend || app.editor.selection_anchor.is_some() {
                app.editor.mode = EditorMode::Normal;
                app.editor.selection_anchor = None;
            } else {
                app.editor.tree_focus = true;
            }
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.editor.mode = EditorMode::Normal;
            app.editor.selection_anchor = None;
            app.editor.tree_focus = true;
        }
        KeyCode::Char('v') => {
            if extend {
                app.editor.mode = EditorMode::Normal;
            } else {
                app.editor.mode = EditorMode::Visual;
                if app.editor.selection_anchor.is_none() {
                    app.editor.selection_anchor =
                        Some((app.editor.cursor_line, app.editor.cursor_col));
                }
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.editor.move_to(
                app.editor.cursor_line,
                app.editor.cursor_col.saturating_sub(1),
                extend,
            );
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let line = (app.editor.cursor_line + 1).min(app.editor.lines.len().saturating_sub(1));
            app.editor.move_to(line, app.editor.cursor_col, extend);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.editor.move_to(
                app.editor.cursor_line.saturating_sub(1),
                app.editor.cursor_col,
                extend,
            );
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let len = app.editor.line_len(app.editor.cursor_line);
            app.editor.move_to(
                app.editor.cursor_line,
                (app.editor.cursor_col + 1).min(len),
                extend,
            );
        }
        KeyCode::Char('w') => {
            if !extend {
                app.editor.selection_anchor = Some((app.editor.cursor_line, app.editor.cursor_col));
            }
            app.editor.move_word_forward(true);
        }
        KeyCode::Char('b') => {
            if !extend {
                app.editor.selection_anchor = Some((app.editor.cursor_line, app.editor.cursor_col));
            }
            app.editor.move_word_backward(true);
        }
        KeyCode::Char('e') => {
            if !extend {
                app.editor.selection_anchor = Some((app.editor.cursor_line, app.editor.cursor_col));
            }
            app.editor.move_word_end(true);
        }
        KeyCode::Char('x') => {
            if app.editor.selection_anchor.is_none() {
                app.editor.selection_anchor = Some((app.editor.cursor_line, 0));
                let len = app.editor.line_len(app.editor.cursor_line);
                app.editor.move_to(app.editor.cursor_line, len, true);
            } else {
                let next =
                    (app.editor.cursor_line + 1).min(app.editor.lines.len().saturating_sub(1));
                let len = app.editor.line_len(next);
                app.editor.move_to(next, len, true);
            }
        }
        KeyCode::Char('%') => {
            let last_line = app.editor.lines.len().saturating_sub(1);
            let last_col = app.editor.line_len(last_line);
            app.editor.selection_anchor = Some((0, 0));
            app.editor.cursor_line = last_line;
            app.editor.cursor_col = last_col;
        }
        KeyCode::Char('g') => app.editor.pending_key = Some('g'),
        KeyCode::Home => app.editor.move_to(app.editor.cursor_line, 0, extend),
        KeyCode::End => {
            let len = app.editor.line_len(app.editor.cursor_line);
            app.editor.move_to(app.editor.cursor_line, len, extend);
        }
        KeyCode::PageUp => {
            app.editor.move_to(
                app.editor.cursor_line.saturating_sub(15),
                app.editor.cursor_col,
                extend,
            );
        }
        KeyCode::PageDown => {
            let line = (app.editor.cursor_line + 15).min(app.editor.lines.len().saturating_sub(1));
            app.editor.move_to(line, app.editor.cursor_col, extend);
        }
        KeyCode::Char('i') => enter_insert(&mut app.editor, false, false, false, None),
        KeyCode::Char('a') => enter_insert(&mut app.editor, true, false, false, None),
        KeyCode::Char('I') => enter_insert(&mut app.editor, false, false, true, None),
        KeyCode::Char('A') => enter_insert(&mut app.editor, false, true, false, None),
        KeyCode::Char('o') => enter_insert(&mut app.editor, false, false, false, Some(true)),
        KeyCode::Char('O') => enter_insert(&mut app.editor, false, false, false, Some(false)),
        KeyCode::Char('d') => {
            if copy_editor_selection(app, false) {
                app.editor.delete_selection();
            } else if app.editor.cursor_col < app.editor.line_len(app.editor.cursor_line) {
                app.editor.push_undo();
                let line = &app.editor.lines[app.editor.cursor_line];
                let s = EditorState::byte_index(line, app.editor.cursor_col);
                let e = EditorState::byte_index(line, app.editor.cursor_col + 1);
                let removed = line[s..e].to_owned();
                app.editor.yank_register = removed.clone();
                app.copy_requested = Some(removed);
                app.editor.delete();
            }
            app.editor.mode = EditorMode::Normal;
        }
        KeyCode::Char('c') => {
            if copy_editor_selection(app, false) {
                app.editor.delete_selection();
            } else if app.editor.cursor_col < app.editor.line_len(app.editor.cursor_line) {
                app.editor.push_undo();
                app.editor.delete();
            }
            enter_insert(&mut app.editor, false, false, false, None);
        }
        KeyCode::Char('y') => {
            if !copy_editor_selection(app, false)
                && let Some(line) = app.editor.lines.get(app.editor.cursor_line)
            {
                let text = format!("{line}\n");
                app.editor.yank_register = text.clone();
                app.copy_requested = Some(text);
            }
            app.editor.mode = EditorMode::Normal;
            app.editor.selection_anchor = None;
        }
        KeyCode::Char('p') => {
            app.editor.mode = EditorMode::Normal;
            paste_text(&mut app.editor, true);
        }
        KeyCode::Char('P') => {
            app.editor.mode = EditorMode::Normal;
            paste_text(&mut app.editor, false);
        }
        KeyCode::Char('u') => {
            app.editor.undo();
        }
        _ => (),
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
        Constraint::Length(if app.editor.tree_has_input() { 3 } else { 1 }),
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

    if let Some(prompt) = &app.editor.tree_prompt {
        let (title, border, line) = match prompt {
            TreePrompt::NewFile(name) => (
                " New file (Enter/Esc) ",
                Color::Green,
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(name.clone(), Style::default().fg(Color::White)),
                    Span::styled("█", Style::default().fg(Color::Green)),
                ]),
            ),
            TreePrompt::NewFolder(name) => (
                " New folder (Enter/Esc) ",
                Color::Cyan,
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(name.clone(), Style::default().fg(Color::White)),
                    Span::styled("█", Style::default().fg(Color::Cyan)),
                ]),
            ),
            TreePrompt::ConfirmDelete { is_dir, name, .. } => (
                if *is_dir {
                    " Delete folder? "
                } else {
                    " Delete file? "
                },
                Color::Red,
                Line::from(vec![
                    Span::styled(
                        format!(" {name}{} ", if *is_dir { "/" } else { "" }),
                        Style::default().fg(Color::Yellow).bold(),
                    ),
                    Span::styled("(y/Enter · n/Esc)", Style::default().fg(Color::Gray)),
                ]),
            ),
        };
        let prompt_block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(border));
        frame.render_widget(Paragraph::new(line).block(prompt_block), footer);
    } else if app.editor.tree_searching {
        let matches = app.editor.tree_search_matches();
        let query_non_empty = !app.editor.tree_search.trim().is_empty();
        let (badge, badge_color) = if !query_non_empty {
            (String::new(), Color::DarkGray)
        } else if matches.is_empty() {
            (" no match".into(), Color::Red)
        } else {
            let pos = matches
                .iter()
                .position(|&idx| idx == app.editor.selected_entry)
                .map_or(1, |i| i + 1);
            (format!(" {pos}/{}", matches.len()), Color::Green)
        };
        let search_line = Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Cyan).bold()),
            Span::styled(app.editor.tree_search.clone(), Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::Cyan)),
            Span::styled(badge, Style::default().fg(badge_color)),
        ]);
        let search_block = Block::bordered()
            .title(" Search ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(Paragraph::new(search_line).block(search_block), footer);
    } else {
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
                        "{folders}d · {}f{} · n/N/x · /",
                        app.editor.entries.len() - folders,
                        if app.editor.entries_truncated {
                            "*"
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
    }

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
            if code_focused {
                let (mode_label, mode_color) = match (app.typing_style, editor.mode) {
                    (crate::preferences::TypingStyle::Regular, _) => (" [REGULAR]", Color::Green),
                    (crate::preferences::TypingStyle::Vim, EditorMode::Insert) => {
                        (" [VIM:INSERT]", Color::Green)
                    }
                    (crate::preferences::TypingStyle::Vim, EditorMode::Visual) => {
                        (" [VIM:VISUAL]", Color::Magenta)
                    }
                    (crate::preferences::TypingStyle::Vim, EditorMode::Normal) => {
                        (" [VIM:NORMAL]", Color::Cyan)
                    }
                    (crate::preferences::TypingStyle::Helix, _)
                        if editor.pending_key == Some('g') =>
                    {
                        (" [HELIX:GOTO]", Color::Yellow)
                    }
                    (crate::preferences::TypingStyle::Helix, EditorMode::Insert) => {
                        (" [HELIX:INSERT]", Color::Green)
                    }
                    (crate::preferences::TypingStyle::Helix, EditorMode::Visual) => {
                        (" [HELIX:SELECT]", Color::Magenta)
                    }
                    (crate::preferences::TypingStyle::Helix, EditorMode::Normal) => {
                        (" [HELIX:NORMAL]", Color::Cyan)
                    }
                };
                spans.push(Span::styled(
                    mode_label,
                    Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
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
    } else if let Some(rel) = editor.open_file.as_deref()
        && crate::imgview::is_image_path(rel)
    {
        let resolved = crate::imgview::resolve_image_path(rel, editor.cwd.as_deref());
        let max_cols = body_area.width.saturating_sub(2);
        let max_rows = body_area.height.saturating_sub(1);
        if let Some(path) = resolved
            && let Some(rendered) = crate::imgview::render_lines(&path, max_cols, max_rows)
        {
            frame.render_widget(Paragraph::new(Text::from(rendered.lines)), body_area);
            frame.render_widget(
                Paragraph::new(format!(
                    " Image preview · {}×{} · Tab/Esc: files",
                    rendered.width, rendered.height
                ))
                .style(Style::default().fg(Color::DarkGray)),
                status_area,
            );
        } else {
            frame.render_widget(
                Paragraph::new(" Could not preview image (unsupported format or remote workspace).")
                    .style(Style::default().fg(Color::Yellow))
                    .wrap(Wrap { trim: false }),
                body_area,
            );
            frame.render_widget(
                Paragraph::new(" Tab/Esc: files").style(Style::default().fg(Color::DarkGray)),
                status_area,
            );
        }
        return;
    } else {
        let visible_rows = body_area.height as usize;
        editor.visible_rows = visible_rows;
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
        let selection = editor.selection_range();
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
            let is_cursor_line = line_idx == editor.cursor_line
                && !editor.tree_focus
                && app.focus == Focus::Workspace;
            let rel_cursor = editor.cursor_col.saturating_sub(editor.scroll_x);
            let chars: Vec<char> = slice.chars().collect();
            let max_cols = if is_cursor_line {
                chars.len().max(rel_cursor + 1)
            } else {
                chars.len()
            };
            let sel_style = Style::default().bg(Color::Rgb(50, 75, 115));
            let cursor_style = if editor.editing {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            };
            for col_offset in 0..max_cols {
                let abs_col = editor.scroll_x + col_offset;
                let ch = chars.get(col_offset).copied().unwrap_or(' ');
                let selected = selection.is_some_and(|((sl, sc), (el, ec))| {
                    if line_idx < sl || line_idx > el {
                        false
                    } else if sl == el {
                        abs_col >= sc && abs_col < ec
                    } else if line_idx == sl {
                        abs_col >= sc
                    } else if line_idx == el {
                        abs_col < ec
                    } else {
                        true
                    }
                });
                let style = if is_cursor_line && col_offset == rel_cursor {
                    cursor_style
                } else if selected {
                    sel_style
                } else {
                    Style::default()
                };
                spans.push(Span::styled(ch.to_string(), style));
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
            if app.typing_style == crate::preferences::TypingStyle::Helix
                && editor.pending_key == Some('g')
            {
                (
                    "GOTO: g top · e end · h/s/l line · t/c/b view · Esc cancel".into(),
                    Color::Yellow,
                )
            } else {
                (
                    format!(
                        "Ln {}, Col {}",
                        editor.cursor_line + 1,
                        editor.cursor_col + 1
                    ),
                    Color::DarkGray,
                )
            }
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

    #[test]
    fn regular_mode_supports_select_copy_cut_paste_and_undo() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            typing_style: crate::preferences::TypingStyle::Regular,
            ..Default::default()
        };
        app.editor.open_file = Some("test.txt".into());
        app.editor.lines = vec!["hello world".into()];
        app.editor.tree_focus = false;

        // Shift+Right selects "hel"
        for _ in 0..3 {
            key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        }
        assert_eq!(app.editor.selected_text().as_deref(), Some("hel"));

        // Ctrl+C copies, Ctrl+A selects all, Ctrl+V replaces all with "hel"
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.editor.yank_register, "hel");
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.editor.selected_text().as_deref(), Some("hello world"));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.editor.lines, vec!["hel"]);

        // Ctrl+Z undoes back to "hello world"
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.editor.lines, vec!["hello world"]);
    }

    #[test]
    fn vim_mode_supports_motions_insert_visual_delete_yank_paste_and_undo() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            typing_style: crate::preferences::TypingStyle::Vim,
            ..Default::default()
        };
        app.editor.open_file = Some("test.txt".into());
        app.editor.lines = vec!["one two".into(), "three".into()];
        app.editor.tree_focus = false;

        // 'w' moves to "two", 'i' enters insert, type "big ", Esc back to normal
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.mode, EditorMode::Insert);
        for ch in "big ".chars() {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.editor.mode, EditorMode::Normal);
        assert_eq!(app.editor.lines[0], "one big two");

        // 'dd' deletes line 0, 'p' pastes it below line "three", 'u' undoes paste
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.lines, vec!["three"]);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.lines, vec!["three", "one big two"]);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.lines, vec!["three"]);
    }

    #[test]
    fn helix_mode_supports_selection_first_motions_delete_yank_paste_and_insert() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            typing_style: crate::preferences::TypingStyle::Helix,
            ..Default::default()
        };
        app.editor.open_file = Some("test.txt".into());
        app.editor.lines = vec!["alpha beta".into()];
        app.editor.tree_focus = false;

        // 'w' selects "alpha ", 'd' deletes it, 'p' pastes it
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.selected_text().as_deref(), Some("alpha "));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.lines, vec!["beta"]);
        assert_eq!(app.editor.yank_register, "alpha ");

        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT),
        );
        assert_eq!(app.editor.lines, vec!["alpha beta"]);
    }

    #[test]
    fn ctrl_s_saves_open_file_in_all_modes() {
        for style in [
            crate::preferences::TypingStyle::Regular,
            crate::preferences::TypingStyle::Vim,
            crate::preferences::TypingStyle::Helix,
        ] {
            let mut app = App {
                focus: Focus::Workspace,
                workspace_tab: 0,
                typing_style: style,
                ..Default::default()
            };
            app.editor.cwd = Some("/repo".into());
            app.editor.open_file = Some("src/lib.rs".into());
            app.editor.lines = vec!["pub fn hi() {}".into()];
            app.editor.tree_focus = false;

            assert!(key(
                &mut app,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ));
            let req = app.editor.request.take().expect("Ctrl+S should queue save");
            assert_eq!(req.payload()["relativePath"], "src/lib.rs");
            assert_eq!(req.payload()["contents"], "pub fn hi() {}");
        }
    }

    #[test]
    fn helix_goto_mode_navigates_file_line_and_viewport() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            typing_style: crate::preferences::TypingStyle::Helix,
            ..Default::default()
        };
        app.editor.open_file = Some("test.txt".into());
        app.editor.lines = (0..30).map(|i| format!("  line {i}")).collect();
        app.editor.tree_focus = false;
        app.editor.scroll_y = 5;
        app.editor.visible_rows = 10;
        app.editor.cursor_line = 10;
        app.editor.cursor_col = 4;

        // 'g' enters goto mode, 's' jumps to first non-whitespace (col 2)
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.pending_key, Some('g'));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.pending_key, None);
        assert_eq!((app.editor.cursor_line, app.editor.cursor_col), (10, 2));

        // 'gl' jumps to line end, 'gh' to line start
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_col, "  line 10".len());
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_col, 0);

        // 'gt', 'gc', 'gb' jump to viewport top/center/bottom
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_line, 5);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_line, 10);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_line, 14);

        // 'ge' jumps to file end, 'gg' to file start
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.cursor_line, 29);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!((app.editor.cursor_line, app.editor.cursor_col), (0, 0));

        // 'g' then Esc cancels goto mode without leaving code view
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.pending_key, Some('g'));
        key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.editor.pending_key, None);
        assert!(!app.editor.tree_focus);
    }

    #[test]
    fn tree_slash_search_finds_cycles_and_opens_file_in_current_folder() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            ..Default::default()
        };
        app.editor.cwd = Some("/repo".into());
        app.editor.tree_focus = true;
        app.editor.entries = vec![
            Entry {
                path: "Cargo.toml".into(),
                kind: "file".into(),
                ignored: None,
            },
            Entry {
                path: "src/main.rs".into(),
                kind: "file".into(),
                ignored: None,
            },
            Entry {
                path: "src/markdown.rs".into(),
                kind: "file".into(),
                ignored: None,
            },
        ];

        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );
        assert!(app.editor.tree_searching);

        // Typing 'm' and 'a' selects main.rs and does not trigger global 'm' composer shortcut.
        for ch in ['m', 'a'] {
            crate::input::handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        assert!(app.focus == Focus::Workspace);
        assert_eq!(app.editor.tree_search, "ma");
        assert_eq!(app.editor.selected_entry, 1);

        // Down cycles to next match (markdown.rs).
        crate::input::handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.editor.selected_entry, 2);

        // Enter closes search and requests opening markdown.rs.
        crate::input::handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.editor.tree_searching);
        let req = app.editor.request.take().unwrap();
        assert_eq!(req.payload()["relativePath"], "src/markdown.rs");
    }

    #[test]
    fn tree_hotkeys_create_file_folder_and_confirm_delete() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            ..Default::default()
        };
        app.editor.cwd = Some("/repo".into());
        app.editor.directory = Some("src".into());
        app.editor.tree_focus = true;
        app.editor.entries = vec![Entry {
            path: "src/main.rs".into(),
            kind: "file".into(),
            ignored: None,
        }];
        app.editor.selected_entry = 1; // index 0 is ".."

        // 'n' opens New file prompt, typing 'q' does not quit, Enter queues CreateFile.
        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        assert_eq!(app.editor.tree_prompt, Some(TreePrompt::NewFile(String::new())));
        for ch in "q.rs".chars() {
            crate::input::handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        assert!(app.quit_dialog.is_none());
        crate::input::handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let req = app.editor.request.take().unwrap();
        app.editor.pending = None;
        assert!(matches!(
            req.operation,
            Operation::CreateFile { ref path, .. } if path == "src/q.rs"
        ));

        // 'N' opens New folder prompt and queues CreateFolder.
        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT),
        );
        for ch in "utils".chars() {
            crate::input::handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        crate::input::handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let req = app.editor.request.take().unwrap();
        app.editor.pending = None;
        assert!(matches!(
            req.operation,
            Operation::CreateFolder { ref path, .. } if path == "src/utils"
        ));

        // 'x' on '..' does nothing; 'x' on src/main.rs prompts and deletes on 'y'.
        app.editor.selected_entry = 0;
        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert!(app.editor.tree_prompt.is_none());

        app.editor.selected_entry = 1;
        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert!(matches!(
            app.editor.tree_prompt,
            Some(TreePrompt::ConfirmDelete { ref path, is_dir: false, .. }) if path == "src/main.rs"
        ));
        crate::input::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        );
        let req = app.editor.request.take().unwrap();
        assert!(matches!(
            req.operation,
            Operation::DeleteEntry { ref path, is_dir: false, .. } if path == "src/main.rs"
        ));
    }
}
