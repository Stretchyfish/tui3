use crate::app::{App, Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Text,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.click_targets.clear();
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
    let [sidebar, main, workspace] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(48),
        Constraint::Percentage(32),
    ])
    .areas(body);
    workspace_panel(frame, app, workspace);
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
    for (area, focus) in [
        (sidebar, Focus::Projects),
        (chat, Focus::Chat),
        (composer, Focus::Composer),
        (model, Focus::Model),
    ] {
        app.click_targets
            .push((area, crate::app::ClickTarget::Pane(focus)));
    }
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
                let elapsed = app.thinking_since.map(|start| start.elapsed());
                let tick = elapsed.map(|duration| duration.as_secs());
                if !app
                    .chat_cache
                    .as_ref()
                    .is_some_and(|(revision, cached_tick, _)| {
                        *revision == app.chat_revision && *cached_tick == tick
                    })
                {
                    let text =
                        crate::chat::conversation(history, &app.runs, elapsed, &app.run_diffs);
                    app.chat_cache = Some((app.chat_revision, tick, text));
                }
                let mut text = app.chat_cache.as_ref().unwrap().2.clone();
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
        let phase = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            % 1200;
        let brightness = if phase < 600 { phase } else { 1200 - phase };
        let running_color = Color::Rgb(
            255,
            (170 + brightness * 85 / 600) as u8,
            (40 + brightness * 80 / 600) as u8,
        );
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(phase / 120) as usize];
        for activity in app.activity.iter().rev().take(5).rev() {
            let running = activity.summary.ends_with("· in progress")
                || activity.summary == "Agent · Working";
            crate::chat::notice(
                &mut content,
                format!(
                    "  {} {}",
                    if running { spinner } else { "•" },
                    activity.summary
                ),
                if running {
                    running_color
                } else {
                    Color::DarkGray
                },
            );
        }
    }
    if app.models.notice_thread.as_deref() == app.selected_thread_id()
        && let Some(notice) = &app.models.notice
    {
        crate::chat::notice(&mut content, notice.clone(), Color::Yellow);
    }
    let block = panel(&title, app.focus == Focus::Chat);
    let inner = block.inner(chat);
    if !app
        .chat_viewport
        .as_ref()
        .is_some_and(|cache| cache.buffer.area.width == inner.width && cache.content == content)
    {
        if let Some(viewport) = &mut app.chat_viewport {
            viewport.update(content, inner.width);
        } else {
            app.chat_viewport = Some(crate::chat::Viewport::new(content, inner.width));
        }
    }
    let viewport = app.chat_viewport.as_ref().unwrap();
    app.chat_max_scroll = viewport.buffer.area.height.saturating_sub(inner.height);
    if app.follow_chat {
        app.chat_scroll = app.chat_max_scroll;
    }
    app.chat_scroll = app.chat_scroll.min(app.chat_max_scroll);
    frame.render_widget(block, chat);
    for y in 0..inner.height {
        for x in 0..inner.width {
            if let Some(cell) = viewport.buffer.cell((x, y.saturating_add(app.chat_scroll))) {
                frame.buffer_mut()[(inner.x + x, inner.y + y)] = cell.clone();
            }
        }
    }
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
    crate::composer::draw(
        frame,
        draft,
        app.draft_cursor,
        app.editing,
        composer_block,
        composer,
    );
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
        Paragraph::new(if app.settings_open {
            " SETTINGS | Esc/s: close | Ctrl+Q: quit "
        } else if app.editing {
            " TYPING | Enter: send | Alt+Enter: newline | Esc: navigate | Ctrl+Q: quit "
        } else if app.focus == Focus::Workspace
            && app.workspace_tab == 0
            && app.diff_view.file_focus
        {
            " ↑↓/jk: file | Enter/click: open | [/]: run | Tab: pane | h: chat "
        } else if app.focus == Focus::Workspace {
            " f/Esc: files | PgUp/PgDn: scroll | [/]: run | Shift+←→: pan | ↓: tools "
        } else if app.focus == Focus::WorkspaceTabs {
            " ←→/hl: highlight | Enter: activate | ↑/k: panel | Tab: pane | q: quit "
        } else if app.focus == Focus::Projects {
            " ↑↓/jk: select | →/l/Enter: details | Tab: pane | s: settings | q: quit "
        } else if app.focus == Focus::Chat {
            " ↓/j: prompt | m: model | Shift+j/k/↑↓: scroll | s: settings | q: quit "
        } else if app.focus == Focus::Model {
            " Enter: choose model | k/↑: prompt | Tab: pane | s: settings | q: quit "
        } else {
            " ↓/j: model | ↑/k: chat | Enter/i: type | m: model | s: settings | q: quit "
        })
        .style(Style::default().fg(Color::DarkGray)),
        footer,
    );
    if app.settings_open {
        app.click_targets.clear();
        settings_panel(frame, app);
    }
    if app.models.open {
        app.click_targets.clear();
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

fn workspace_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let names = ["Diffs", "Code", "Files", "Terminal", "Git", "Side chat"];
    let letters = ["D", "C", "F", "T", "G", "S"];
    let [content, selector] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(area);
    app.click_targets
        .push((content, crate::app::ClickTarget::Pane(Focus::Workspace)));
    if app.workspace_tab == 0 {
        crate::diff::draw(frame, app, content);
    } else {
        frame.render_widget(
            Paragraph::new(letters[app.workspace_tab])
                .centered()
                .block(panel(
                    names[app.workspace_tab],
                    app.focus == Focus::Workspace,
                )),
            content,
        );
    }
    let boxes = Layout::horizontal([Constraint::Ratio(1, 6); 6])
        .spacing(1)
        .split(selector);
    for (index, rect) in boxes.iter().enumerate() {
        app.click_targets
            .push((*rect, crate::app::ClickTarget::Workspace(index)));
        let selected = app.workspace_tab == index;
        let available = rect.width.saturating_sub(4) as usize;
        let label = if available >= names[index].len() {
            names[index].to_owned()
        } else if available > 2 {
            format!(
                "{}…",
                names[index].chars().take(available - 1).collect::<String>()
            )
        } else {
            names[index].chars().take(available).collect()
        };
        selector_box(
            frame,
            *rect,
            &label,
            selected,
            app.workspace_highlight == index && app.focus == Focus::WorkspaceTabs,
        );
    }
}

fn settings_panel(frame: &mut Frame, app: &mut App) {
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
    let block = panel(" Settings ", true)
        .title_bottom(" Esc/s: close ")
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [heading, selector, details] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Min(0),
    ])
    .areas(inner);
    frame.render_widget(Paragraph::new("Typing style"), heading);
    let options = Layout::horizontal([Constraint::Ratio(1, 3); 3])
        .spacing(1)
        .split(selector);
    use crate::preferences::TypingStyle;
    for (style, option) in [TypingStyle::Regular, TypingStyle::Vim, TypingStyle::Helix]
        .into_iter()
        .zip(options.iter())
    {
        let selected = app.typing_style == style;
        app.click_targets
            .push((*option, crate::app::ClickTarget::TypingStyle(style)));
        selector_box(
            frame,
            *option,
            style.label(),
            selected,
            app.style_highlight == style,
        );
    }
    let model = app
        .current_model()
        .map(|model| model.model.as_str())
        .unwrap_or("No thread selected");
    let content = format!(
        "Left/Right or h/l: highlight; Enter: activate\nBlue outer outline: highlighted; green outline: active\nSaved automatically. Vim/Helix editing behavior is planned.\n\nConnection\n{}\n\nCurrent model\n{}",
        app.connection, model,
    );
    frame.render_widget(Paragraph::new(content).wrap(Wrap { trim: false }), details);
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

fn selector_box(frame: &mut Frame, area: Rect, label: &str, active: bool, highlighted: bool) {
    let outer = Block::bordered()
        .borders(if area.height <= 3 {
            Borders::LEFT | Borders::RIGHT
        } else {
            Borders::ALL
        })
        .border_style(
            Style::default()
                .fg(if highlighted {
                    Color::Blue
                } else {
                    Color::Black
                })
                .bg(Color::Black),
        );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let block = Block::bordered().border_style(
        Style::default()
            .fg(if active {
                Color::Green
            } else {
                Color::DarkGray
            })
            .bg(Color::Black),
    );
    frame.render_widget(
        Paragraph::new(label)
            .centered()
            .block(block)
            .style(Style::default().fg(Color::Gray).bg(Color::Black)),
        inner,
    );
}

pub(crate) fn panel(title: &str, focused: bool) -> Block<'_> {
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
    #[ignore = "performance benchmark; run with --release --ignored --nocapture"]
    fn performance_large_thread_navigation() {
        use std::time::{Duration, Instant};
        let mut app = App {
            focus: Focus::Workspace,
            ..Default::default()
        };
        let mut items = Vec::new();
        for n in 0..200 {
            let id = format!("r{n}");
            app.runs.insert(
                id.clone(),
                serde_json::from_value(
                    serde_json::json!({"id":id,"ordinal":n+1,"status":"completed"}),
                )
                .unwrap(),
            );
            for m in 0..5 {
                items.push(serde_json::json!({"position":n*10+m,"item":{"id":format!("m{n}-{m}"),"type":"assistant_message","runId":id,"text":"Progress message with enough text to wrap across several terminal lines.\nSecond line of content."}}));
                items.push(serde_json::json!({"position":n*10+m+5,"item":{"id":format!("f{n}-{m}"),"type":"file_change","runId":id,"fileName":format!("src/file{m}.rs")}}));
            }
            app.run_diffs
                .insert(id, [("src/main.rs".into(), (5, 2))].into());
        }
        app.transcript = Some(
            serde_json::from_value(
                serde_json::json!({"threadId":"t1","title":"Large thread","items":items}),
            )
            .unwrap(),
        );
        app.history_thread = Some("t1".into());
        let patch = (0..100).map(|n| format!("diff --git a/file{n}.rs b/file{n}.rs\n--- a/file{n}.rs\n+++ b/file{n}.rs\n@@ -1,100 +1,100 @@\n{}", "-old\n+new\n".repeat(100))).collect::<String>();
        app.diff_view.patches.insert("r199".into(), patch.into());
        let mut terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        let mut times = Vec::new();
        for _ in 0..35 {
            let started = Instant::now();
            app.diff_view.scroll(true, 1);
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            times.push(started.elapsed());
            app.diff_view.file_focus = false; // Benchmark actual diff scrolling after the list opens.
        }
        times.remove(0); // Measure interaction after the initial projection is cached.
        times.sort();
        let p95 = times[times.len() * 95 / 100];
        println!(
            "large-thread redraw: median {:?}, p95 {:?}, max {:?}",
            times[times.len() / 2],
            p95,
            times.last().unwrap()
        );
        if !cfg!(debug_assertions) {
            assert!(
                p95 < Duration::from_millis(17),
                "p95 frame budget exceeded: {p95:?}"
            );
        }
        let mut streaming = Vec::new();
        for n in 0..20 {
            let started = Instant::now();
            let item = app
                .transcript
                .as_mut()
                .unwrap()
                .items
                .iter_mut()
                .rev()
                .find(|entry| entry.item.kind == "assistant_message")
                .unwrap();
            item.item.text = Some(format!(
                "Streaming update {n}\n{}",
                "New output. ".repeat(n + 1)
            ));
            item.item.streaming = true;
            app.chat_revision += 1;
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            streaming.push(started.elapsed());
        }
        streaming.sort();
        let streaming_p95 = streaming[streaming.len() * 95 / 100];
        println!(
            "streaming reflow: median {:?}, p95 {:?}",
            streaming[streaming.len() / 2],
            streaming_p95
        );
        if !cfg!(debug_assertions) {
            assert!(
                streaming_p95 < Duration::from_millis(17),
                "streaming frame budget exceeded: {streaming_p95:?}"
            );
        }
    }

    #[test]
    fn mouse_activates_rendered_selectors_and_settings_blocks_background_clicks() {
        use crate::app::ClickTarget;
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::default();
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let area = app
            .click_targets
            .iter()
            .find(|(_, target)| matches!(target, ClickTarget::Workspace(3)))
            .unwrap()
            .0;
        let click = |area: Rect| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        };
        crate::input::handle_mouse(&mut app, click(area));
        assert_eq!(app.workspace_tab, 3);
        assert!(app.focus == Focus::WorkspaceTabs);
        app.settings_open = true;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(
            app.click_targets
                .iter()
                .all(|(_, target)| matches!(target, ClickTarget::TypingStyle(_)))
        );
        let area = app
            .click_targets
            .iter()
            .find(|(_, target)| {
                matches!(
                    target,
                    ClickTarget::TypingStyle(crate::preferences::TypingStyle::Helix)
                )
            })
            .unwrap()
            .0;
        crate::input::handle_mouse(&mut app, click(area));
        assert_eq!(app.typing_style, crate::preferences::TypingStyle::Helix);
    }

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
        let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
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
