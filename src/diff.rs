//! Read-only per-run checkpoint diff viewer.
use crate::{
    app::{App, Focus},
    history::Run,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct DiffView {
    pub patches: HashMap<String, Patch>,
    pub file: usize,
    pub file_focus: bool,
    displayed_run: Option<String>,
    pub errors: HashMap<String, String>,
    pub selected: Option<String>,
    pub scroll: u16,
    pub max_scroll: u16,
    pub horizontal: u16,
}

impl DiffView {
    pub fn run_id<'a>(&'a self, runs: &'a HashMap<String, Run>) -> Option<&'a str> {
        self.selected
            .as_deref()
            .filter(|id| runs.contains_key(*id))
            .or_else(|| {
                runs.values()
                    .filter(|run| run.status == "completed" && run.ordinal.is_some())
                    .max_by_key(|run| run.ordinal)
                    .map(|run| run.id.as_str())
            })
    }

    pub fn scroll(&mut self, down: bool, amount: u16) {
        self.scroll = if down {
            self.scroll.saturating_add(amount).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(amount)
        };
    }

    pub fn select_run(&mut self, runs: &HashMap<String, Run>, next: bool) {
        let mut choices: Vec<_> = runs
            .values()
            .filter(|run| run.status == "completed" && run.ordinal.is_some())
            .collect();
        choices.sort_by_key(|run| run.ordinal);
        let Some(index) = choices
            .iter()
            .position(|run| Some(run.id.as_str()) == self.run_id(runs))
        else {
            return;
        };
        let index = if next {
            (index + 1).min(choices.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        self.selected = Some(choices[index].id.clone());
        self.scroll = 0;
        self.horizontal = 0;
    }
}

pub struct Patch {
    pub files: Vec<DiffFile>,
}
impl From<&str> for Patch {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
pub struct DiffFile {
    pub added: u64,
    pub removed: u64,
    pub name: String,
    pub lines: Vec<Line<'static>>,
}
impl From<String> for Patch {
    fn from(patch: String) -> Self {
        let mut files: Vec<DiffFile> = Vec::new();
        for line in patch.lines() {
            if line.starts_with("diff --git ") || files.is_empty() {
                let name = line.rsplit_once(" b/").map_or("Diff", |(_, name)| name);
                files.push(DiffFile {
                    name: name.to_owned(),
                    added: 0,
                    removed: 0,
                    lines: Vec::new(),
                });
            }
            let file = files.last_mut().unwrap();
            if let Some(name) = line.strip_prefix("+++ b/") {
                file.name = name.to_owned();
            }
            if file.name == "Diff"
                && let Some(name) = line.strip_prefix("--- a/")
            {
                file.name = name.to_owned();
            }
            let style = if line.starts_with("diff --git ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                Style::default().fg(Color::Cyan).bold()
            } else if line.starts_with('+') {
                Style::default().fg(Color::Green)
            } else if line.starts_with('-') {
                Style::default().fg(Color::Red)
            } else if line.starts_with("@@") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Gray)
            };
            if line.starts_with('+') && !line.starts_with("+++ ") {
                file.added += 1;
            }
            if line.starts_with('-') && !line.starts_with("--- ") {
                file.removed += 1;
            }
            file.lines.push(Line::styled(line.to_owned(), style));
        }
        Self { files }
    }
}

impl DiffView {
    pub fn select_file(&mut self, index: usize) {
        self.file = index;
        self.scroll = 0;
        self.horizontal = 0;
    }
    pub fn move_file(&mut self, runs: &HashMap<String, Run>, down: bool) {
        let count = self
            .run_id(runs)
            .and_then(|id| self.patches.get(id))
            .map_or(0, |patch| patch.files.len());
        if count > 0 {
            self.select_file(if down {
                (self.file + 1).min(count - 1)
            } else {
                self.file.saturating_sub(1)
            });
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};
    let id = app.diff_view.run_id(&app.runs).map(str::to_owned);
    if app.diff_view.displayed_run != id {
        app.diff_view.select_file(0);
        app.diff_view.file_focus = true;
        app.diff_view.displayed_run = id.clone();
    }
    let run = id.as_ref().and_then(|id| app.runs.get(id));
    let title = run
        .and_then(|run| run.ordinal)
        .map_or(" Diffs ".into(), |n| format!(" Diffs · run {n} "));
    let block = super::ui::panel(&title, app.focus == Focus::Workspace);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if let Some(patch) = id.as_ref().and_then(|id| app.diff_view.patches.get(id))
        && !patch.files.is_empty()
    {
        app.diff_view.file = app.diff_view.file.min(patch.files.len() - 1);
        let [diff_area, files_area] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                .areas(inner);
        let files_block = super::ui::panel(
            " Files ",
            app.focus == Focus::Workspace && app.diff_view.file_focus,
        );
        let files_inner = files_block.inner(files_area);
        frame.render_widget(files_block, files_area);
        {
            let [heading, list, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(files_inner);
            frame.render_widget(
                Paragraph::new(format!(
                    "{} changed files · {}/{}",
                    patch.files.len(),
                    app.diff_view.file + 1,
                    patch.files.len()
                ))
                .style(Style::default().fg(Color::Cyan)),
                heading,
            );
            let visible = list.height as usize;
            let offset = app
                .diff_view
                .file
                .saturating_sub(visible / 2)
                .min(patch.files.len().saturating_sub(visible));
            for (row, (index, file)) in patch
                .files
                .iter()
                .enumerate()
                .skip(offset)
                .take(visible)
                .enumerate()
            {
                let selected = index == app.diff_view.file;
                let style = if selected {
                    Style::default().fg(Color::White).bg(Color::DarkGray).bold()
                } else {
                    Style::default().fg(Color::Gray)
                };
                let counts = if list.width >= 24 {
                    format!(" +{} -{}", file.added, file.removed)
                } else {
                    String::new()
                };
                let available = list.width.saturating_sub(counts.len() as u16 + 2) as usize;
                let name = short_path(&file.name, available);
                let padding =
                    available.saturating_sub(unicode_width::UnicodeWidthStr::width(name.as_str()));
                let line = Line::from(vec![
                    Span::raw(format!(
                        "{}{}{:<padding$}",
                        if selected { "› " } else { "  " },
                        name,
                        "",
                        padding = padding
                    )),
                    Span::styled(
                        if counts.is_empty() {
                            String::new()
                        } else {
                            format!(" +{}", file.added)
                        },
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        if counts.is_empty() {
                            String::new()
                        } else {
                            format!(" -{}", file.removed)
                        },
                        Style::default().fg(Color::Red),
                    ),
                ])
                .style(style);
                let row = Rect::new(list.x, list.y + row as u16, list.width, 1);
                frame.render_widget(Paragraph::new(line).style(style), row);
                app.click_targets
                    .push((row, crate::app::ClickTarget::DiffFile(index)));
            }
            frame.render_widget(
                Paragraph::new("←/h: diff").style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        }
        let diff_block = super::ui::panel(
            " Patch ",
            app.focus == Focus::Workspace && !app.diff_view.file_focus,
        );
        let diff_inner = diff_block.inner(diff_area);
        frame.render_widget(diff_block, diff_area);
        app.click_targets
            .push((diff_area, crate::app::ClickTarget::DiffBody));
        let [heading, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(diff_inner);
        frame.render_widget(
            Paragraph::new(format!(
                "{}\n+{} -{} · →/l: files",
                patch.files[app.diff_view.file].name,
                patch.files[app.diff_view.file].added,
                patch.files[app.diff_view.file].removed
            ))
            .style(Style::default().fg(Color::Cyan)),
            heading,
        );
        let file = &patch.files[app.diff_view.file];
        app.diff_view.max_scroll = file
            .lines
            .len()
            .saturating_sub(body.height as usize)
            .min(u16::MAX as usize) as u16;
        app.diff_view.scroll = app.diff_view.scroll.min(app.diff_view.max_scroll);
        let lines = file
            .lines
            .iter()
            .skip(app.diff_view.scroll as usize)
            .take(body.height as usize)
            .cloned()
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(lines)).scroll((0, app.diff_view.horizontal)),
            body,
        );
        return;
    }
    let message = if let Some(id) = id {
        if app.diff_view.patches.contains_key(&id) {
            "No changes in this run.".into()
        } else if let Some(error) = app.diff_view.errors.get(&id) {
            format!("Diff unavailable\n{error}\n\nPress r to retry.")
        } else {
            "Loading checkpoint diff from T3…".into()
        }
    } else if app.history_thread.is_none() {
        "Select a thread to view its diffs.".into()
    } else {
        "No completed runs yet. Diffs appear after a run finishes.".into()
    };
    frame.render_widget(Paragraph::new(message), inner);
}

fn short_path(path: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if path.width() <= width {
        return path.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 1;
    let tail: String = path
        .chars()
        .rev()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used <= width
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_only_selected_file_and_mouse_switches_files() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App {
            focus: Focus::Workspace,
            ..Default::default()
        };
        app.runs.insert(
            "r1".into(),
            serde_json::from_value(serde_json::json!({"id":"r1","ordinal":1,"status":"completed"}))
                .unwrap(),
        );
        app.diff_view.patches.insert("r1".into(), "diff --git a/one.rs b/one.rs\n+++ b/one.rs\n@@ -1 +1 @@\n+FIRST_ONLY\ndiff --git a/two.rs b/two.rs\n+++ b/two.rs\n@@ -1 +1 @@\n+SECOND_ONLY".into());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &mut app, frame.area()))
            .unwrap();
        let content = |terminal: &Terminal<TestBackend>| {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        assert!(content(&terminal).contains("2 changed files"));
        assert!(content(&terminal).contains("one.rs"));
        assert!(content(&terminal).contains("FIRST_ONLY"));
        assert!(!content(&terminal).contains("SECOND_ONLY"));
        let row = app
            .click_targets
            .iter()
            .find(|(_, target)| matches!(target, crate::app::ClickTarget::DiffFile(1)))
            .unwrap()
            .0;
        crate::input::handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        terminal
            .draw(|frame| draw(frame, &mut app, frame.area()))
            .unwrap();
        assert!(!content(&terminal).contains("FIRST_ONLY"));
        assert!(content(&terminal).contains("SECOND_ONLY"));
    }

    #[test]
    fn selects_latest_completed_run_and_browses_in_ordinal_order() {
        let runs: HashMap<_, Run> = [
            ("old", 1, "completed"),
            ("new", 2, "completed"),
            ("active", 3, "running"),
        ]
        .into_iter()
        .map(|(id, ordinal, status)| {
            (
                id.into(),
                serde_json::from_value(serde_json::json!({
                    "id":id, "ordinal":ordinal, "status":status
                }))
                .unwrap(),
            )
        })
        .collect();
        let mut view = DiffView::default();
        assert_eq!(view.run_id(&runs), Some("new"));
        view.scroll = 10;
        view.horizontal = 8;
        view.select_run(&runs, false);
        assert_eq!(view.run_id(&runs), Some("old"));
        assert_eq!((view.scroll, view.horizontal), (0, 0));
        view.select_run(&runs, true);
        assert_eq!(view.run_id(&runs), Some("new"));
    }

    #[test]
    fn renders_server_patch_with_diff_colors_and_empty_state() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App {
            focus: Focus::Workspace,
            ..Default::default()
        };
        app.runs.insert(
            "r1".into(),
            serde_json::from_value(serde_json::json!({
                "id":"r1", "ordinal":1,"status":"completed"
            }))
            .unwrap(),
        );
        app.diff_view.patches.insert(
            "r1".into(),
            "diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new".into(),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal
            .draw(|frame| draw(frame, &mut app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer
                .content
                .iter()
                .any(|cell| cell.symbol() == "+" && cell.fg == Color::Green)
        );
        assert!(
            buffer
                .content
                .iter()
                .any(|cell| cell.symbol() == "-" && cell.fg == Color::Red)
        );
        app.diff_view
            .patches
            .insert("r1".into(), String::new().into());
        terminal
            .draw(|frame| draw(frame, &mut app, frame.area()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("No changes in this run."));
    }
}
