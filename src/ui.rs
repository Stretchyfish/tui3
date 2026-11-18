use crate::app::{App, Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Text,
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(format!(" tui3 | {}", app.connection)).bold(),
        header,
    );
    let [sidebar, main] =
        Layout::horizontal([Constraint::Percentage(28), Constraint::Percentage(72)]).areas(body);
    let project_list = sidebar;
    let [chat, composer, controls] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(5),
        Constraint::Length(3),
    ])
    .areas(main);
    let [model, session, weekly] = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(30),
        Constraint::Percentage(30),
    ])
    .areas(controls);
    let title = if app.catalog_error.is_some() {
        " Projects / threads (refresh failed) "
    } else {
        " Projects / threads "
    };
    if app.rows.is_empty() {
        let message = if let Some(error) = &app.catalog_error {
            format!("Could not load projects.\n\n{error}\n\nRetrying automatically.")
        } else if app.catalog.is_some() {
            "No projects found on this T3 server.".into()
        } else if app.authenticated {
            "Loading projects and threads…".into()
        } else {
            "Waiting for T3 sign-in.\n\nPress p to pair with the local server.".into()
        };
        frame.render_widget(
            Paragraph::new(message)
                .wrap(Wrap { trim: false })
                .block(panel(title, app.focus == Focus::Projects)),
            project_list,
        );
    } else {
        let items = app.rows.iter().map(|row| ListItem::new(row.label.as_str()));
        frame.render_stateful_widget(
            List::new(items)
                .block(panel(title, app.focus == Focus::Projects))
                .highlight_symbol("› ")
                .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
            project_list,
            &mut app.sidebar_state,
        );
    }
    let error = app
        .catalog_error
        .as_ref()
        .map(|error| format!("\n\nRefresh failed: {error}\nShowing the last loaded list."))
        .unwrap_or_default();
    let (title, mut content) = if app.history_thread.is_some() {
        let title = app
            .transcript
            .as_ref()
            .map(|history| {
                format!(
                    " Chat: {} [{}] ",
                    history.title,
                    if app.history_live {
                        "live"
                    } else {
                        "reconnecting"
                    }
                )
            })
            .unwrap_or_else(|| " Chat ".into());
        let content = match (&app.transcript, &app.history_error) {
            (Some(history), error) => {
                let mut text = crate::chat::conversation(history, &app.runs);
                if let Some(error) = error {
                    crate::chat::notice(
                        &mut text,
                        format!(
                            "Refresh failed: {error}\nShowing last loaded history; retrying automatically."
                        ),
                        Color::Yellow,
                    );
                }
                text
            }
            (None, Some(error)) => Text::from(format!(
                "Could not load chat history.\n\n{error}\n\nRetrying automatically."
            )),
            (None, None) => Text::from("Loading chat history…"),
        };
        (title, content)
    } else {
        (
            " Selection ".into(),
            Text::from(format!(
                "{}\n\n{}\n\n{}{}",
                app.connection,
                app.pairing_message,
                app.selection_details(),
                error
            )),
        )
    };
    if !app.activity.is_empty() {
        crate::chat::notice(&mut content, "Agent activity", Color::Cyan);
        for activity in app.activity.iter().rev().take(5).rev() {
            crate::chat::notice(
                &mut content,
                format!("  • {}", activity.summary),
                Color::DarkGray,
            );
        }
    }
    if app.thinking_since.is_some() {
        let elapsed = app
            .thinking_since
            .map_or_else(
                || app.worked_duration.unwrap_or_default(),
                |start| start.elapsed(),
            )
            .as_secs();
        let timer = if elapsed >= 3600 {
            format!(
                "{:02}:{:02}:{:02}",
                elapsed / 3600,
                elapsed / 60 % 60,
                elapsed % 60
            )
        } else {
            format!("{:02}:{:02}", elapsed / 60, elapsed % 60)
        };
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(elapsed as usize) % 10];
        let label = format!("{spinner} AGENT · working for {timer}");
        crate::chat::notice(&mut content, label, Color::Green);
    }
    if let Some(status) = app
        .history_thread
        .as_ref()
        .and_then(|id| app.send_status.get(id))
    {
        crate::chat::notice(&mut content, status.clone(), Color::DarkGray);
    }
    if app.models.notice_thread.as_deref() == app.selected_thread_id()
        && let Some(notice) = &app.models.notice
    {
        crate::chat::notice(&mut content, notice.clone(), Color::Yellow);
    }
    let block = panel(&title, app.focus == Focus::Chat);
    let inner = block.inner(chat);
    let paragraph = Paragraph::new(content).wrap(Wrap { trim: false });
    app.chat_max_scroll = paragraph
        .line_count(inner.width)
        .saturating_sub(usize::from(inner.height))
        .min(usize::from(u16::MAX)) as u16;
    if app.follow_chat {
        app.chat_scroll = app.chat_max_scroll;
    }
    app.chat_scroll = app.chat_scroll.min(app.chat_max_scroll);
    frame.render_widget(paragraph.scroll((app.chat_scroll, 0)).block(block), chat);
    let draft = &app.draft;
    let status = app.send_notice.as_deref().or_else(|| {
        app.draft_thread
            .as_ref()
            .and_then(|id| app.send_status.get(id).map(String::as_str))
    });
    let target = app
        .draft_thread
        .as_ref()
        .and_then(|id| {
            app.catalog
                .as_ref()?
                .threads
                .iter()
                .find(|thread| &thread.id == id)
        })
        .map(|thread| thread.title.as_str());
    let composer_title = format!(
        " Message{}: {} ",
        if app.editing {
            " (typing)"
        } else if app.focus == Focus::Composer {
            " (Enter or i to type)"
        } else {
            ""
        },
        target.unwrap_or("select a thread")
    );
    let composer_block = panel(&composer_title, app.focus == Focus::Composer);
    let composer_block = if let Some(status) = status {
        composer_block.title_bottom(status)
    } else {
        composer_block
    };
    let draft_paragraph = Paragraph::new(draft.as_str()).wrap(Wrap { trim: false });
    let composer_inner = composer_block.inner(composer);
    let draft_scroll = draft_paragraph
        .line_count(composer_inner.width)
        .saturating_sub(usize::from(composer_inner.height))
        .min(usize::from(u16::MAX)) as u16;
    frame.render_widget(
        draft_paragraph
            .scroll((draft_scroll, 0))
            .block(composer_block),
        composer,
    );
    if app.editing && composer_inner.width > 0 && composer_inner.height > 0 {
        use unicode_width::UnicodeWidthChar;
        let mut line = 0usize;
        let mut column = 0usize;
        for ch in app.draft.chars().take(app.draft_cursor) {
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += ch.width().unwrap_or(0);
                if column >= usize::from(composer_inner.width) {
                    line += 1;
                    column = 0;
                }
            }
        }
        let visible_line = line.saturating_sub(usize::from(draft_scroll));
        if visible_line < usize::from(composer_inner.height) {
            frame.set_cursor_position((
                composer_inner
                    .x
                    .saturating_add(column.min(usize::from(composer_inner.width - 1)) as u16),
                composer_inner.y.saturating_add(visible_line as u16),
            ));
        }
    }
    let current = app
        .current_model()
        .map(|selection| selection.model.clone())
        .unwrap_or_else(|| "Select a thread".into());
    let notice = if app.models.notice_thread.as_deref() == app.selected_thread_id() {
        app.models.notice.as_ref()
    } else {
        None
    };
    let model_block = panel(
        if app.focus == Focus::Model {
            " Model (Enter: change) "
        } else {
            " Model "
        },
        app.focus == Focus::Model,
    );
    let model_block = if notice.is_some() {
        model_block.title_bottom(if app.models.pending.is_some() {
            " Changing… "
        } else {
            " See chat for status "
        })
    } else {
        model_block
    };
    frame.render_widget(Paragraph::new(current).block(model_block), model);
    usage_panel(frame, session, " Session left ", app, "session");
    usage_panel(frame, weekly, " Week left ", app, "weekly");
    frame.render_widget(
        Paragraph::new(if app.editing {
            " TYPING | Enter: send | Alt+Enter: newline | Esc: navigate | Ctrl+Q: quit "
        } else if app.focus == Focus::Projects {
            " ↑↓/jk: select | →/l/Enter: details | Tab: pane | p: pair | q: quit "
        } else if app.focus == Focus::Chat {
            " ↓/j: prompt | m: model | Shift+j/k/↑↓: scroll | PgUp/PgDn: scroll | q: quit "
        } else if app.focus == Focus::Model {
            " Enter: choose model | k/↑: prompt | Tab: pane | q: quit "
        } else {
            " ↓/j: model | ↑/k: chat | Enter/i: type | m: model | Tab: next | q: quit "
        })
        .style(Style::default().fg(Color::DarkGray)),
        footer,
    );
    if app.models.open {
        let [_, center, _] = Layout::vertical([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .areas(frame.area());
        let [_, area, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(center);
        frame.render_widget(Clear, area);
        let block = panel(" Choose model — for future messages ", true)
            .style(Style::default().bg(Color::Black).fg(Color::White));
        let hint = app
            .models
            .error
            .as_deref()
            .unwrap_or("↑↓/jk: select | Enter: apply | Esc: cancel | r: refresh");
        let block = block.title_bottom(hint);
        if app.models.loading || app.models.choices.is_empty() {
            let text = if app.models.loading {
                "Loading models from T3…"
            } else {
                app.models.error.as_deref().unwrap_or(
                    "No available models. Check your providers in T3, then press r to refresh.",
                )
            };
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
                area,
            );
        } else {
            let items: Vec<_> = app
                .models
                .choices
                .iter()
                .map(|choice| {
                    let current = app.current_model().is_some_and(|current| {
                        current.instance_id == choice.selection.instance_id
                            && current.model == choice.selection.model
                    });
                    ListItem::new(format!(
                        "{}{}",
                        if current { "* " } else { "  " },
                        choice.label
                    ))
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
                area,
                &mut app.models.list_state,
            );
        }
    }
}

fn usage_panel(frame: &mut Frame, area: Rect, title: &str, app: &App, kind: &str) {
    let remaining = app
        .current_model()
        .and_then(|model| app.usage.remaining(&model.instance_id, &model.model, kind));
    let mut block = panel(title, false);
    let (text, color) = if let Some(remaining) = remaining {
        let percent = if remaining.percent > 0.0 && remaining.percent < 0.1 {
            "<0.1%".into()
        } else if remaining.percent.fract().abs() < 0.001 {
            format!("{:.0}%", remaining.percent)
        } else {
            format!("{:.1}%", remaining.percent)
        };
        if remaining.stale {
            block = block.title_bottom(" stale ");
        }
        let suffix = if block.inner(area).width >= 13 {
            " left"
        } else {
            ""
        };
        let text = format!(
            "{} {percent}{suffix}{}",
            crate::usage::circle(remaining.percent),
            if remaining.stale { "*" } else { "" }
        );
        let color = if remaining.percent <= 10.0 {
            Color::Red
        } else if remaining.percent <= 25.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        (text, color)
    } else {
        let text = if app.current_model().is_none() {
            "—"
        } else if !app.usage.loaded && app.usage.error.is_none() {
            "Loading…"
        } else {
            "N/A"
        };
        (text.into(), Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(color))
            .block(block),
        area,
    );
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::bordered()
        .title(title)
        .border_style(Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_compact_model_and_two_remaining_usage_boxes() {
        let mut app = App::default();
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        catalog.threads[0].model_selection = Some(crate::models::ModelSelection {
            instance_id: "codex".into(),
            model: "gpt-test".into(),
            options: None,
        });
        app.update_catalog(catalog);
        app.move_selection(true);
        app.usage.apply(
            crate::usage::from_config_event(crate::usage::tests::snapshot())
                .unwrap()
                .unwrap(),
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Session left"));
        assert!(text.contains("Week left"));
        assert!(text.contains("◕ 75% left"));
        assert!(text.contains("○ 0% left"));
        // Three-line row: title, one content line, bottom border.
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .find(|cell| cell.symbol() == "◕")
                .unwrap()
                .fg,
            Color::Green
        );
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .find(|cell| cell.symbol() == "○")
                .unwrap()
                .fg,
            Color::Red
        );
        app.usage.error = Some("disconnected".into());
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("stale"));
        assert!(text.contains("75% left*"));
    }

    #[test]
    fn empty_composer_has_no_placeholder_but_keeps_header_hint() {
        let mut app = App {
            focus: Focus::Composer,
            ..App::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Enter or i to type"));
        assert!(!text.contains("Press Enter or i to type"));
        assert!(!text.contains("Enter sends; Alt+Enter"));
    }

    #[test]
    fn renders_model_picker_and_handles_small_terminal_sizes() {
        let mut app = App {
            authenticated: true,
            focus: Focus::Model,
            ..Default::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.models.choices = crate::models::choices(crate::models::tests::catalog()).unwrap();
        app.open_models();
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Choose model"));
        assert!(text.contains("Model B"));
        for (width, height) in [(1, 1), (10, 3), (30, 8)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        }
    }

    #[test]
    fn renders_history_scrolls_to_latest_and_preserves_manual_scrolling() {
        let mut app = App {
            authenticated: true,
            ..App::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        let mut history = crate::history::tests::transcript("t1");
        history.items[0].item.text =
            Some(format!("{}\nLATEST MESSAGE", "Older line\n".repeat(100)));
        app.apply_history("t1", Ok(history));
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("LATEST MESSAGE"));
        assert_eq!(app.chat_scroll, app.chat_max_scroll);
        app.chat_scroll = 0;
        app.follow_chat = false;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Question"));
        assert!(!text.contains("LATEST MESSAGE"));
        assert_eq!(app.chat_scroll, 0);
    }

    #[test]
    fn scrolls_sidebar_to_selection_and_handles_small_terminals() {
        let mut app = App::default();
        let mut catalog = crate::catalog::tests::snapshot();
        for index in 0..40 {
            let mut thread = catalog.threads[0].clone();
            thread.id = format!("extra-{index}");
            catalog.threads.push(thread);
        }
        catalog.normalize();
        app.update_catalog(catalog);
        app.sidebar_state.select(Some(app.rows.len() - 1));
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.sidebar_state.offset() > 0);
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Second"));
        for (width, height) in [(1, 1), (10, 3), (30, 8)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        }
    }
}
