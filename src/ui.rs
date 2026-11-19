use crate::app::{App, Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
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
        Paragraph::new(format!(
            " tui3 | {} | {}",
            app.connection, app.startup.event.message
        ))
        .bold(),
        header,
    );
    let narrow = body.width < 100;
    let show_workspace = app.workspace_visible && !narrow;
    let (sidebar, main, workspace) = if show_workspace {
        let side = app.workspace_width.clamp(20, 55);
        let [sidebar, main, workspace] = Layout::horizontal([
            Constraint::Percentage(20),
            Constraint::Percentage(80 - side),
            Constraint::Percentage(side),
        ])
        .areas(body);
        (sidebar, main, workspace)
    } else {
        let [sidebar, main] =
            Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)])
                .areas(body);
        (sidebar, main, Rect::default())
    };
    if show_workspace {
        workspace_panel(frame, app, workspace);
    }
    if narrow {
        frame.render_widget(
            Paragraph::new("Narrow terminal: workspace hidden (press w to toggle)"),
            Rect::new(body.x, body.y, body.width, 1),
        );
    }
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
        let host = app.environment_label.as_deref();
        let items = app.rows.iter().map(|row| {
            let mut line = match &row.selection {
                crate::catalog::Selection::Thread(id) if app.is_thread_running(id) => {
                    if let Some(rest) = row.label.strip_prefix("  · ") {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::styled("●", Style::default().fg(Color::Green)),
                            Span::raw(" "),
                            Span::raw(rest),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled("● ", Style::default().fg(Color::Green)),
                            Span::raw(row.label.as_str()),
                        ])
                    }
                }
                _ => Line::from(row.label.as_str()),
            };
            if let (crate::catalog::Selection::Project(_), Some(host)) = (&row.selection, host) {
                line.push_span(format!(" · {host}").dark_gray());
            }
            ListItem::new(line)
        });
        frame.render_stateful_widget(
            List::new(items)
                .block(panel(title, app.focus == Focus::Projects))
                .highlight_symbol("› ")
                .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
            project_list,
            &mut app.sidebar_state,
        );
        let first_visible = app.sidebar_state.offset();
        let visible_rows = project_list.height.saturating_sub(2) as usize;
        for index in first_visible..app.rows.len().min(first_visible + visible_rows) {
            let row_y = project_list.y + 1 + (index - first_visible) as u16;
            app.click_targets.push((
                Rect::new(project_list.x, row_y, project_list.width, 1),
                crate::app::ClickTarget::SidebarRow(index),
            ));
        }
    }
    app.sidebar_menu_anchor = (
        project_list.right(),
        project_list.y.saturating_add(1).saturating_add(
            app.sidebar_state
                .selected()
                .unwrap_or(0)
                .saturating_sub(app.sidebar_state.offset()) as u16,
        ),
    );
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
                if app.usage_exhausted()
                    && app
                        .selected_thread_id()
                        .is_some_and(|id| app.previous_messages.contains_key(id))
                {
                    crate::chat::notice(
                        &mut text,
                        "Usage limit reached. When usage is available again, press r to resend the previous message.",
                        Color::Yellow,
                    );
                }
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
    let queued_runs = app
        .runs
        .values()
        .filter(|run| run.status == "queued")
        .count();
    let title = if queued_runs > 0 {
        format!("{} · {queued_runs} queued ", title.trim_end())
    } else {
        title
    };
    if queued_runs > 0 {
        crate::chat::notice(
            &mut content,
            format!("{queued_runs} message(s) queued · waiting for the active run"),
            Color::Yellow,
        );
    }
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
    let chat_inner = block.inner(chat);
    let image_count = app
        .draft_thread
        .as_ref()
        .and_then(|id| app.images.get(id))
        .map_or(0, Vec::len);
    let attachment_height = if image_count > 0 {
        image_count.min(3) as u16 + 2
    } else {
        0
    };
    let [inner, attachment_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(attachment_height.min(chat_inner.height.saturating_sub(1))),
    ])
    .areas(chat_inner);
    app.chat_text_area = inner;
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
    let queue_status =
        (queued_runs > 0).then(|| format!("{queued_runs} queued · waiting for the active run"));
    let status = app
        .send_notice
        .as_deref()
        .or(queue_status.as_deref())
        .or_else(|| {
            app.draft_thread
                .as_ref()
                .and_then(|id| app.send_status.get(id).map(String::as_str))
        });
    let image_count = app
        .draft_thread
        .as_ref()
        .and_then(|id| app.images.get(id))
        .map_or(0, Vec::len);
    let composer_title = format!(
        " Message{}{}{} ",
        if app.is_fresh_context() {
            " · Fresh context"
        } else {
            ""
        },
        if image_count > 0 {
            format!(" · {image_count} image(s) attached")
        } else {
            String::new()
        },
        if app.editing {
            " (typing)"
        } else if app.focus == Focus::Composer {
            " (Enter/Space or i to type · Shift+D: dictate)"
        } else {
            ""
        }
    );
    let composer_block = panel(&composer_title, app.focus == Focus::Composer);
    let composer_block = if let Some(indicator) = app.dictation.indicator(std::time::Instant::now())
    {
        let color = if app.dictation.listening() {
            Color::Red
        } else {
            Color::Yellow
        };
        composer_block.title_top(
            Line::from(indicator)
                .right_aligned()
                .style(Style::default().fg(color).bold()),
        )
    } else {
        composer_block
    };
    let composer_block = if let Some(status) = status {
        composer_block.title_bottom(status)
    } else {
        composer_block
    };
    app.composer_text_area = composer_block.inner(composer);
    crate::composer::draw(
        frame,
        draft,
        app.draft_cursor,
        app.editing,
        composer_block,
        composer,
    );
    if let Some(anchor) = app.draft_selection_anchor {
        crate::composer::highlight_selection(
            frame,
            draft,
            app.draft_cursor,
            anchor,
            app.composer_text_area,
        );
    }
    let slash_matches = app.matching_slash_commands();
    if !slash_matches.is_empty()
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
        && app.quit_dialog.is_none()
        && app.thread_menu.is_none()
    {
        let height = (slash_matches.len() as u16 + 2).min(composer.y.max(3));
        let width = composer.width.min(72).max(20);
        let popup = Rect::new(composer.x, composer.y.saturating_sub(height), width, height);
        frame.render_widget(Clear, popup);
        let block =
            panel(" Commands ", true).title_bottom(" ↑/↓ select · Enter/Tab apply · Esc close ");
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let selected = app
            .slash_highlight
            .min(slash_matches.len().saturating_sub(1));
        for (index, command) in slash_matches.iter().enumerate() {
            if index as u16 >= inner.height {
                break;
            }
            let row_area = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            let is_selected = index == selected;
            let name_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Cyan).bold()
            };
            let desc_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::Gray)
            };
            let line = Line::from(vec![
                Span::styled(format!(" /{: <7}", command.name), name_style),
                Span::styled(command.description, desc_style),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(if is_selected {
                    Style::default().bg(Color::Cyan)
                } else {
                    Style::default()
                }),
                row_area,
            );
            app.click_targets
                .push((row_area, crate::app::ClickTarget::SlashCommand(index)));
        }
    }
    if image_count > 0 {
        let attachments_block = Block::bordered()
            .title(format!(" Unsent attachments ({image_count}) "))
            .title_bottom(" Click × to remove · Ctrl+Backspace: clear ")
            .border_style(Style::default().fg(Color::Cyan));
        let rows = attachments_block.inner(attachment_area);
        frame.render_widget(attachments_block, attachment_area);
        if let Some(images) = app.draft_thread.as_ref().and_then(|id| app.images.get(id)) {
            for (index, image) in images.iter().take(rows.height as usize).enumerate() {
                let row = Rect::new(rows.x, rows.y + index as u16, rows.width, 1);
                let name = image["name"].as_str().unwrap_or("image");
                let kib = image["sizeBytes"].as_u64().unwrap_or(0).div_ceil(1024);
                let more =
                    if index + 1 == rows.height as usize && image_count > rows.height as usize {
                        format!(" · +{} more", image_count - rows.height as usize)
                    } else {
                        String::new()
                    };
                frame.render_widget(
                    Paragraph::new(format!("[×] {name} · {kib} KiB{more}"))
                        .style(Style::default().fg(Color::Cyan)),
                    row,
                );
                app.click_targets
                    .push((row, crate::app::ClickTarget::RemoveAttachment(index)));
            }
        }
    }
    let current = app
        .current_model()
        .map(|selection| {
            let reasoning = app
                .models
                .choices
                .iter()
                .find(|choice| {
                    choice.selection.instance_id == selection.instance_id
                        && choice.selection.model == selection.model
                })
                .and_then(|choice| choice.reasoning.as_ref())
                .and_then(|descriptor| {
                    selection.option_value(&descriptor.id).map(|value| {
                        descriptor
                            .options
                            .iter()
                            .find(|option| option.id == value)
                            .map_or(value, |option| option.label.as_str())
                            .to_owned()
                    })
                })
                .or_else(|| selection.option_value("reasoningEffort").map(str::to_owned));
            reasoning.map_or_else(
                || selection.model.clone(),
                |level| format!("{} · {level}", selection.model),
            )
        })
        .unwrap_or_else(|| "Select a thread".into());
    let notice = if app.models.notice_thread.as_deref() == app.selected_thread_id() {
        app.models.notice.as_ref()
    } else {
        None
    };
    let model_block = panel(
        if app.focus == Focus::Model {
            " Model (Enter/Space: change) "
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
    app.click_targets
        .push((session, crate::app::ClickTarget::Pane(Focus::SessionUsage)));
    app.click_targets
        .push((weekly, crate::app::ClickTarget::Pane(Focus::WeeklyUsage)));
    let footer_hint = if app.settings_open {
        " SETTINGS | Esc/s: close | Ctrl+Q: quit "
    } else if app.editing {
        " TYPING | Enter: queue | Ctrl+Enter: steer | Ctrl+C/V: copy/paste | Shift+Enter: newline | Shift+arrows: select | Ctrl+Shift+←→: words "
    } else if app.focus == Focus::Workspace && app.workspace_tab == 3 {
        " GIT | ↑↓/jk: control | Enter/Space: open/confirm | g: generate | r: refresh | Tab: pane "
    } else if app.focus == Focus::Workspace && app.workspace_tab == 0 && app.diff_view.file_focus {
        " ↑↓/jk: file | ←/h/Enter: patch | [/]: run | Tab: pane "
    } else if app.focus == Focus::Workspace && app.workspace_tab == 0 {
        " w: toggle wrap | PgUp/PgDn: scroll | Shift+←→: pan when unwrapped | ↑: tools "
    } else if app.focus == Focus::Workspace {
        " →/l: editor | PgUp/PgDn: scroll | [/]: run | Shift+←→: pan | ↑: tools "
    } else if app.focus == Focus::WorkspaceTabs {
        " ←→/hl: tabs | Enter/Space: activate | ↓/j: panel "
    } else if app.focus == Focus::Projects {
        " ↑↓/jk: select | →/l: chat | Enter/Space: menu | Tab: pane | s: settings | o: setup "
    } else if app.focus == Focus::Chat {
        " ↓/j or m: Message | Shift+j/k/↑↓: scroll | s: settings "
    } else if app.focus == Focus::Model {
        " Enter/Space: choose model | k/↑: prompt | Tab: pane | s: settings "
    } else {
        " ↓/j: model | ↑/k: chat | Enter/Space/i: type | Shift+D: dictate | m: Message | s: settings "
    };
    let [hint_area, shortcut_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(footer.width.min(72))])
            .areas(footer);
    frame.render_widget(
        Paragraph::new(footer_hint).style(Style::default().fg(Color::DarkGray)),
        hint_area,
    );
    frame.render_widget(
        Paragraph::new("Shift+D Dictate | Ctrl+J / Shift+T Terminal | w Panel { } Resize | q Quit")
            .right_aligned()
            .style(Style::default().fg(Color::DarkGray)),
        shortcut_area,
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
        let hint =
            app.models.error.as_deref().unwrap_or(
                "←→/hl: panel | ↑↓/jk: choose | Enter/Space: reasoning/apply | Esc: cancel",
            );
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
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let [model_area, reasoning_area] =
                Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .spacing(1)
                    .areas(inner);
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
                    .block(panel(" Model ", !app.models.reasoning_focus))
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
                model_area,
                &mut app.models.list_state,
            );
            let reasoning = app
                .models
                .list_state
                .selected()
                .and_then(|index| app.models.choices.get(index))
                .and_then(|choice| {
                    choice.reasoning.as_ref().map(|descriptor| {
                        let value = choice.reasoning_value();
                        let choices = descriptor
                            .options
                            .iter()
                            .map(|option| {
                                if value == Some(option.id.as_str()) {
                                    format!("[{}]", option.label)
                                } else {
                                    option.label.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!("{}\n\n{choices}", descriptor.label)
                    })
                })
                .unwrap_or_else(|| "No reasoning levels for this model".into());
            frame.render_widget(
                Paragraph::new(reasoning)
                    .wrap(Wrap { trim: false })
                    .block(panel(
                        " Reasoning · ↑/↓ to change ",
                        app.models.reasoning_focus,
                    )),
                reasoning_area,
            );
        }
    }
    if app.startup.open {
        app.click_targets.clear();
        crate::startup::draw(frame, app);
    }
    if app.terminal_overlay_open {
        app.click_targets.clear();
        let [_, center, _] = Layout::vertical([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(frame.area());
        let [_, area, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(center);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("Terminal panel placeholder")
                .block(panel(" Terminal ", true).title_bottom(" Ctrl+J, Shift+T or Esc: close ")),
            area,
        );
    }
    app.screen_buffer = app.screen_selection.map(|_| frame.buffer_mut().clone());
    if let Some((a, b)) = app.screen_selection {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let area = app.screen_selection_area.unwrap_or(frame.area());
        for y in area.y..area.bottom() {
            let row_right = if app.screen_selection_area.is_some() {
                (area.x..area.right())
                    .rev()
                    .find(|&x| {
                        app.screen_buffer
                            .as_ref()
                            .and_then(|buffer| buffer.cell((x, y)))
                            .is_some_and(|cell| !cell.symbol().trim().is_empty())
                    })
                    .map_or(area.x, |x| x + 1)
            } else {
                area.right()
            };
            for x in area.x..row_right {
                if (y, x) >= start && (y, x) < end {
                    frame.buffer_mut()[(x, y)].set_bg(Color::Blue);
                }
            }
        }
    }
    if let Some((column, row)) = app.context_menu {
        let screen = frame.area();
        let width = screen.width.min(27);
        let height = screen.height.min(4);
        let area = Rect::new(
            column.min(screen.right().saturating_sub(width)),
            row.min(screen.bottom().saturating_sub(height)),
            width,
            height,
        );
        app.context_menu_area = area;
        let copy_color = if app
            .selected_draft()
            .or_else(|| app.selected_screen())
            .is_some()
        {
            Color::White
        } else {
            Color::DarkGray
        };
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::styled(" Copy selection", Style::default().fg(copy_color)),
                Line::styled(" Paste to Message", Style::default().fg(Color::White)),
            ]))
            .block(Block::bordered().title(" Clipboard ")),
            area,
        );
    } else {
        app.context_menu_area = Rect::default();
    }
    if let Some(menu) = app.thread_menu.clone() {
        draw_thread_menu(frame, app, &menu);
    }
    if let Some(dialog) = app.quit_dialog {
        draw_quit_dialog(frame, app, dialog);
    }
}

fn draw_quit_dialog(frame: &mut Frame, app: &mut App, dialog: crate::app::QuitDialog) {
    app.click_targets.clear();
    let screen = frame.area();
    let width = screen.width.min(54).max(20);
    let height = screen.height.min(9).max(7);
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = panel(" Close tui3 ", true)
        .title_bottom(" Enter/Space confirm · y/n · ←/→ switch · Esc cancel ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [prompt, _, buttons] = Layout::vertical([
        Constraint::Min(2),
        Constraint::Length(1),
        Constraint::Length(3),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new("Shut down the local T3 server on close?")
            .centered()
            .wrap(Wrap { trim: false }),
        prompt,
    );
    let [_, yes_area, _, no_area, _, cancel_area, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(11),
        Constraint::Length(2),
        Constraint::Length(10),
        Constraint::Length(2),
        Constraint::Length(12),
        Constraint::Fill(1),
    ])
    .areas(buttons);
    let yes_style = if dialog.shutdown_server {
        Style::default().fg(Color::Black).bg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Green)
    };
    let no_style = if !dialog.shutdown_server {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::White)
    };
    frame.render_widget(
        Paragraph::new("Yes (y)")
            .centered()
            .style(yes_style)
            .block(Block::bordered().border_style(yes_style)),
        yes_area,
    );
    frame.render_widget(
        Paragraph::new("No (n)")
            .centered()
            .style(no_style)
            .block(Block::bordered().border_style(no_style)),
        no_area,
    );
    frame.render_widget(
        Paragraph::new("Cancel")
            .centered()
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::bordered().border_style(Style::default().fg(Color::DarkGray))),
        cancel_area,
    );
    app.click_targets
        .push((yes_area, crate::app::ClickTarget::QuitChoice(true)));
    app.click_targets
        .push((no_area, crate::app::ClickTarget::QuitChoice(false)));
    app.click_targets
        .push((cancel_area, crate::app::ClickTarget::QuitCancel));
}

fn draw_thread_menu(frame: &mut Frame, app: &mut App, menu: &crate::app::ThreadMenu) {
    use crate::app::{ClickTarget, ThreadMenuMode};
    app.click_targets.clear();
    let screen = frame.area();
    let width = screen.width.min(40);
    let height = screen.height.min(match menu.mode {
        ThreadMenuMode::ProjectOptions => 5,
        ThreadMenuMode::Options => 6,
        _ => 9,
    });
    let (x, y) = app.sidebar_menu_anchor;
    let area = Rect::new(
        x.min(screen.right().saturating_sub(width)),
        y.min(screen.bottom().saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let title = format!(" {} ", menu.original_title);
    let block = panel(&title, true).title_bottom(" Enter/Space: select · Esc: back ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let row = |offset: u16| {
        Rect::new(
            inner.x,
            inner.y + offset,
            inner.width,
            if offset < inner.height { 1 } else { 0 },
        )
    };
    match menu.mode {
        ThreadMenuMode::ProjectOptions => {
            frame.render_widget(
                Paragraph::new(if menu.busy {
                    "Creating…"
                } else {
                    "New thread (n)"
                })
                .style(Style::default().fg(Color::Black).bg(Color::Cyan)),
                row(0),
            );
            app.click_targets
                .push((row(0), ClickTarget::ThreadMenuConfirm));
        }
        ThreadMenuMode::Options => {
            for (index, label) in ["Rename thread (r)", "Delete thread (d)"]
                .iter()
                .enumerate()
            {
                let style = if menu.selected_option == index {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                frame.render_widget(Paragraph::new(*label).style(style), row(index as u16));
                app.click_targets
                    .push((row(index as u16), ClickTarget::ThreadMenuOption(index)));
            }
        }
        ThreadMenuMode::Rename => {
            frame.render_widget(
                Paragraph::new(format!("{}▏", menu.rename_input))
                    .style(Style::default().fg(Color::Cyan)),
                row(2),
            );
            frame.render_widget(
                Paragraph::new(if menu.busy { "Saving…" } else { "Save" })
                    .style(Style::default().fg(Color::Green)),
                row(3),
            );
            app.click_targets
                .push((row(3), ClickTarget::ThreadMenuConfirm));
        }
        ThreadMenuMode::ConfirmDelete => {
            frame.render_widget(Paragraph::new("Delete this thread?"), row(2));
            frame.render_widget(
                Paragraph::new(if menu.busy {
                    "Deleting…"
                } else {
                    "Delete (y)"
                })
                .style(Style::default().fg(Color::Red)),
                row(3),
            );
            app.click_targets
                .push((row(3), ClickTarget::ThreadMenuConfirm));
        }
    }
    if matches!(
        menu.mode,
        ThreadMenuMode::Rename | ThreadMenuMode::ConfirmDelete
    ) {
        frame.render_widget(
            Paragraph::new("Back").style(Style::default().fg(Color::DarkGray)),
            row(4),
        );
        app.click_targets
            .push((row(4), ClickTarget::ThreadMenuCancel));
    }
    if let Some(error) = &menu.error {
        frame.render_widget(
            Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false }),
            row(inner.height.saturating_sub(1)),
        );
    }
}

fn workspace_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    app.click_targets
        .push((area, crate::app::ClickTarget::Pane(Focus::Workspace)));
    let names = ["Diffs", "Editor", "Terminal", "Git"];
    let letters = ["D", "E", "t", "g"];
    let active_tab = app.workspace_tab.min(names.len() - 1);
    let [selector, body] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    let border = if matches!(app.focus, Focus::Workspace | Focus::WorkspaceTabs) {
        Color::Blue
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(border));
    let content = block.inner(body);
    frame.render_widget(block, body);
    app.click_targets
        .push((content, crate::app::ClickTarget::Pane(Focus::Workspace)));
    if active_tab == 0 {
        crate::diff::draw(frame, app, content);
    } else if active_tab == 3 {
        crate::git::draw(frame, app, content);
    } else {
        frame.render_widget(Paragraph::new(letters[active_tab]).centered(), content);
    }
    let boxes = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(selector);
    for (index, rect) in boxes.iter().enumerate() {
        app.click_targets
            .push((*rect, crate::app::ClickTarget::Workspace(index)));
    }
    // Neighbors share a border column. Draw the active tab last so its
    // open corners and accent also own the shared edges.
    let order = (0..boxes.len())
        .filter(|&index| index != active_tab)
        .chain(std::iter::once(active_tab));
    for index in order {
        let mut rect = boxes[index];
        if index + 1 < boxes.len() && rect.width > 0 {
            rect.width = rect.width.saturating_add(1);
        }
        let selected = active_tab == index;
        draw_workspace_tab(
            frame,
            rect,
            names[index],
            selected,
            app.workspace_highlight == index && app.focus == Focus::WorkspaceTabs,
            index,
            boxes.len(),
        );
    }
    if matches!(app.focus, Focus::Workspace | Focus::WorkspaceTabs)
        && area.width > 0
        && area.height > 0
    {
        let buffer = frame.buffer_mut();
        for x in area.x..area.right() {
            buffer[(x, area.y)].set_fg(Color::Blue);
            buffer[(x, area.bottom() - 1)].set_fg(Color::Blue);
        }
        for y in area.y..area.bottom() {
            buffer[(area.x, y)].set_fg(Color::Blue);
            buffer[(area.right() - 1, y)].set_fg(Color::Blue);
        }
    }
}

fn draw_workspace_tab(
    frame: &mut Frame,
    area: Rect,
    name: &str,
    active: bool,
    highlighted: bool,
    index: usize,
    count: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buffer = frame.buffer_mut();
    let border = if highlighted {
        Color::Blue
    } else if active {
        Color::Green
    } else {
        Color::DarkGray
    };
    let first = index == 0;
    let last = index + 1 == count;
    let top_style = Style::default().fg(border);
    buffer.set_string(area.x, area.y, if first { "╭" } else { "┬" }, top_style);
    for x in area.x.saturating_add(1)..area.x.saturating_add(area.width).saturating_sub(1) {
        buffer[(x, area.y)].set_char('─').set_style(top_style);
    }
    if area.width > 1 {
        buffer.set_string(
            area.x + area.width - 1,
            area.y,
            if last { "╮" } else { "┬" },
            top_style,
        );
    }
    if area.height > 1 {
        let y = area.y + 1;
        buffer.set_string(area.x, y, "│", top_style);
        if area.width > 1 {
            buffer.set_string(area.x + area.width - 1, y, "│", top_style);
        }
        let available = area.width.saturating_sub(2) as usize;
        let label = if name.chars().count() > available {
            name.chars().take(available).collect::<String>()
        } else {
            name.to_owned()
        };
        let label_width = label.chars().count() as u16;
        let x = area.x + 1 + area.width.saturating_sub(2 + label_width) / 2;
        if available > 0 {
            buffer.set_string(
                x,
                y,
                label,
                Style::default()
                    .fg(if active { Color::Green } else { Color::Gray })
                    .bold(),
            );
        }
    }
    if area.height > 2 && !active {
        let y = area.y + area.height - 1;
        let left = if first { "├" } else { "┴" };
        let right = if last { "┤" } else { "┴" };
        buffer.set_string(area.x, y, left, top_style);
        for x in area.x.saturating_add(1)..area.x.saturating_add(area.width).saturating_sub(1) {
            buffer[(x, y)].set_char('─').set_style(top_style);
        }
        if area.width > 1 {
            buffer.set_string(area.x + area.width - 1, y, right, top_style);
        }
    }
    if area.height > 2 && active {
        let y = area.bottom() - 1;
        buffer[(area.x, y)]
            .set_symbol(if first { "│" } else { "╯" })
            .set_style(top_style);
        if area.width > 1 {
            buffer[(area.right() - 1, y)]
                .set_symbol(if last { "│" } else { "╰" })
                .set_style(top_style);
        }
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
        .title_bottom(" r: recheck clipboard · Esc/s: close ")
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [
        heading,
        selector,
        startup_heading,
        startup_selector,
        wrap_heading,
        wrap_selector,
        clipboard_heading,
        clipboard_result,
        details,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(2),
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
            app.settings_row == 0 && app.style_highlight == style,
        );
    }
    frame.render_widget(
        Paragraph::new("Start T3 when tui3 launches"),
        startup_heading,
    );
    let options = Layout::horizontal([Constraint::Ratio(1, 2); 2])
        .spacing(1)
        .split(startup_selector);
    for (index, value) in [true, false].into_iter().enumerate() {
        selector_box(
            frame,
            options[index],
            if value { "On" } else { "Off" },
            app.auto_start_t3 == value,
            app.settings_row == 1 && app.auto_start_highlight == value,
        );
        app.click_targets
            .push((options[index], crate::app::ClickTarget::AutoStart(value)));
    }
    frame.render_widget(Paragraph::new("Wrap diff lines"), wrap_heading);
    let wrap_options = Layout::horizontal([Constraint::Ratio(1, 2); 2])
        .spacing(1)
        .split(wrap_selector);
    for (index, value) in [true, false].into_iter().enumerate() {
        selector_box(
            frame,
            wrap_options[index],
            if value { "On" } else { "Off" },
            app.diff_view.wrap == value,
            app.settings_row == 2 && app.diff_wrap_highlight == value,
        );
    }
    frame.render_widget(Paragraph::new("Desktop clipboard"), clipboard_heading);
    let (status, color) = match &app.clipboard_status {
        crate::app::ClipboardStatus::Checking => {
            ("Checking clipboard access…".to_owned(), Color::Yellow)
        }
        crate::app::ClipboardStatus::Available => (
            "Available · clipboard read access works".to_owned(),
            Color::Green,
        ),
        crate::app::ClipboardStatus::Unavailable(error) => {
            (format!("Unavailable · {error}"), Color::Red)
        }
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: false }),
        clipboard_result,
    );
    let model = app
        .current_model()
        .map(|model| model.model.as_str())
        .unwrap_or("No thread selected");
    let content = format!(
        "↑↓/jk: setting · ←→/hl: highlight · Enter: apply\nBlue: highlighted; green: active. Saved automatically.\nVim/Helix editing behavior is planned.\n\nConnection\n{}\n\nCurrent model\n{}",
        app.connection, model,
    );
    frame.render_widget(Paragraph::new(content).wrap(Wrap { trim: false }), details);
}

fn usage_panel(frame: &mut Frame, area: Rect, title: &str, app: &App, kind: &str) {
    let remaining = app
        .current_model()
        .and_then(|model| app.usage.remaining(&model.instance_id, &model.model, kind));
    let focused = app.focus
        == if kind == "session" {
            Focus::SessionUsage
        } else {
            Focus::WeeklyUsage
        };
    let mut block = panel(title, focused);
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
        let reset = remaining
            .resets_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|reset| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_secs() as i64);
                let secs = (reset.timestamp() - now).max(0);
                format!(" · {}h {}m", secs / 3600, (secs % 3600) / 60)
            })
            .unwrap_or_default();
        let text = format!(
            "{} {percent}{suffix}{}{}",
            crate::usage::circle(remaining.percent),
            if remaining.stale { "*" } else { "" },
            reset
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
    fn settings_shows_clipboard_access_state() {
        let mut app = App::default();
        app.settings_open = true;
        app.clipboard_status =
            crate::app::ClipboardStatus::Unavailable("No display session".into());
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let visible = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("Desktop clipboard"));
        assert!(visible.contains("Unavailable · No display session"));
        app.clipboard_status = crate::app::ClipboardStatus::Available;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let visible = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("Available · clipboard read access works"));
    }

    #[test]
    fn queued_run_is_visible_in_chat_and_message_panels() {
        let mut app = App::default();
        app.history_thread = Some("t1".into());
        app.transcript = Some(crate::history::tests::transcript("t1"));
        let run = serde_json::from_value::<crate::history::Run>(
            serde_json::json!({"id":"r2","status":"queued"}),
        )
        .unwrap();
        app.runs.insert(run.id.clone(), run);
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let area = terminal.backend().buffer().area;
        let all = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("queued · waiting for the active run"));
        assert!(all.contains("· 1 queued"));
    }

    #[test]
    fn attachment_remove_click_uses_rendered_chat_target_and_preserves_prompt() {
        let mut app = App::default();
        app.draft_thread = Some("thread".into());
        app.draft = "Keep this prompt".into();
        app.images.insert(
            "thread".into(),
            vec![serde_json::json!({
                "id": "image", "name": "clipboard.png", "sizeBytes": 1024
            })],
        );
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let target = app
            .click_targets
            .iter()
            .find_map(|(rect, target)| {
                matches!(target, crate::app::ClickTarget::RemoveAttachment(0)).then_some(*rect)
            })
            .unwrap();
        crate::input::handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: target.x,
                row: target.y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        );
        assert!(app.images["thread"].is_empty());
        assert_eq!(app.draft, "Keep this prompt");
    }

    #[test]
    fn clicking_a_thread_then_enter_opens_its_compact_menu() {
        use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let project_row = app
            .click_targets
            .iter()
            .find_map(|(rect, target)| {
                matches!(target, crate::app::ClickTarget::SidebarRow(0)).then_some(*rect)
            })
            .unwrap();
        crate::input::handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: project_row.x,
                row: project_row.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        crate::input::handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(matches!(
            app.thread_menu.as_ref().map(|menu| menu.mode),
            Some(crate::app::ThreadMenuMode::ProjectOptions)
        ));
        app.thread_menu = None;
        let row = app
            .click_targets
            .iter()
            .find_map(|(rect, target)| {
                matches!(target, crate::app::ClickTarget::SidebarRow(1)).then_some(*rect)
            })
            .unwrap();

        crate::input::handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        crate::input::handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert!(app.focus == Focus::Projects);
        assert_eq!(app.thread_menu.as_ref().unwrap().thread_id, "t1");
    }

    #[test]
    fn workspace_tabs_are_above_content() {
        use crate::app::ClickTarget;
        let mut app = App::default();
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let tabs = app
            .click_targets
            .iter()
            .find_map(|(rect, target)| matches!(target, ClickTarget::Workspace(0)).then_some(*rect))
            .unwrap();
        let content = app
            .click_targets
            .iter()
            .filter_map(|(rect, target)| {
                matches!(target, ClickTarget::Pane(Focus::Workspace)).then_some(*rect)
            })
            .min_by_key(|rect| rect.height)
            .unwrap();
        assert!(tabs.y + tabs.height <= content.y);
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Diffs"));
    }

    #[test]
    fn project_rows_show_the_environment_machine_label() {
        let mut app = App {
            environment_label: Some("test-machine".into()),
            ..Default::default()
        };
        app.rows = vec![
            crate::catalog::Row {
                selection: crate::catalog::Selection::Project("p1".into()),
                label: "tui3".into(),
            },
            crate::catalog::Row {
                selection: crate::catalog::Selection::Thread("t1".into()),
                label: "  · Conversation".into(),
            },
        ];
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("tui3 · test-machine"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("Conversation · test-machine"))
        );
    }

    #[test]
    fn quit_dialog_renders_server_shutdown_prompt() {
        let mut app = App {
            quit_dialog: Some(crate::app::QuitDialog {
                shutdown_server: true,
            }),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Shut down the local T3 server on close?"));
        assert!(text.contains("Yes (y)"));
        assert!(text.contains("No (n)"));
        assert!(
            app.click_targets
                .iter()
                .any(|(_, target)| matches!(target, crate::app::ClickTarget::QuitChoice(true)))
        );
        assert!(
            app.click_targets
                .iter()
                .any(|(_, target)| matches!(target, crate::app::ClickTarget::QuitChoice(false)))
        );
    }

    #[test]
    fn running_threads_show_green_indicator_in_project_menu() {
        let mut catalog = crate::catalog::tests::snapshot();
        let mut running_thread = catalog.threads[0].clone();
        running_thread.id = "t-running".into();
        running_thread.title = "Active work".into();
        running_thread.status = "running".into();
        catalog.threads.push(running_thread);
        let mut app = App::default();
        app.update_catalog(catalog);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut found_green_dot = false;
        for y in 0..buffer.area.height {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if line.contains("Active work") {
                found_green_dot = (0..buffer.area.width).any(|x| {
                    let cell = &buffer[(x, y)];
                    cell.symbol() == "●" && cell.fg == Color::Green
                });
            }
            if line.contains("Conversation") {
                assert!(!line.contains('●'));
            }
        }
        assert!(found_green_dot);
    }

    #[test]
    fn thread_menu_renders_options_rename_and_delete() {
        let mut app = App {
            thread_menu: Some(crate::app::ThreadMenu {
                thread_id: "t1".into(),
                original_title: "Conversation".into(),
                mode: crate::app::ThreadMenuMode::Options,
                selected_option: 0,
                rename_input: "Conversation".into(),
                busy: false,
                error: None,
            }),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Conversation"));
        assert!(!text.contains("Thread options"));
        assert!(text.contains("Rename thread (r)"));
        assert!(text.contains("Delete thread (d)"));
    }

    #[test]
    fn slash_command_popup_and_fresh_context_badge_render() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.sync_history_selection();
        app.focus = Focus::Composer;
        app.editing = true;
        app.draft = "/".into();
        app.draft_cursor = 1;
        app.fresh_context_threads.insert("t1".into());

        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Commands"));
        assert!(text.contains("/clear"));
        assert!(text.contains("/model"));
        assert!(text.contains("Fresh context"));
        assert!(
            app.click_targets
                .iter()
                .any(|(_, target)| matches!(target, crate::app::ClickTarget::SlashCommand(0)))
        );
    }

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
        assert!(app.click_targets.iter().all(|(_, target)| matches!(
            target,
            ClickTarget::TypingStyle(_) | ClickTarget::AutoStart(_)
        )));
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
        assert!(text.contains("Enter/Space or i to type"));
        assert!(!text.contains("Press Enter/Space or i to type"));
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

    #[test]
    fn composer_shows_dictation_indicator_when_recording_or_transcribing() {
        let mut app = App::default();
        app.dictation.phase = crate::dictation::Phase::Recording;
        app.dictation.started = Some(std::time::Instant::now());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Dictating"));

        app.dictation.phase = crate::dictation::Phase::Transcribing;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Transcribing"));
    }
}
