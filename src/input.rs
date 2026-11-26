use crate::app::{App, Focus};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn copy_selection(app: &mut App) {
    let Some(text) = app.selected_draft().or_else(|| app.selected_screen()) else {
        app.send_notice = Some("Select text before copying.".into());
        return;
    };
    app.copy_requested = Some(text);
    app.send_notice = Some("Copying selected text…".into());
}

pub fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    use crate::app::ClickTarget;
    use crossterm::event::{MouseButton, MouseEventKind};
    let position = ratatui::layout::Position::new(mouse.column, mouse.row);
    app.pointer = Some(position);
    if app.git.commit_modal
        && app.git.confirm.is_none()
        && !app.git.picking
        && !app.settings_open
        && !app.terminal_overlay_open
        && app.git.message_area.contains(position)
    {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.git.scroll_message(true, 3);
                return;
            }
            MouseEventKind::ScrollUp => {
                app.git.scroll_message(false, 3);
                return;
            }
            _ => {}
        }
    }
    if app.git.commit_modal
        && app.git.confirm.is_none()
        && !app.git.picking
        && !app.git.picking_mode
        && !app.settings_open
        && !app.terminal_overlay_open
        && app.git.files_area.contains(position)
    {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.git.files_scroll = app.git.files_scroll.saturating_add(3);
                return;
            }
            MouseEventKind::ScrollUp => {
                app.git.files_scroll = app.git.files_scroll.saturating_sub(3);
                return;
            }
            _ => {}
        }
    }
    if app.terminal_overlay_open {
        if let Some(session) = &app.local_terminal.session {
            match mouse.kind {
                MouseEventKind::ScrollUp => session.scroll(3),
                MouseEventKind::ScrollDown => session.scroll(-3),
                _ => (),
            }
        }
        return;
    }

    let point = (mouse.row, mouse.column);
    if mouse.kind == MouseEventKind::Down(MouseButton::Right) {
        app.context_menu = Some((mouse.column, mouse.row));
        return;
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.context_menu.take().is_some() {
        let area = app.context_menu_area;
        if area.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
            if mouse.row == area.y + 1 {
                copy_selection(app);
            } else if mouse.row == area.y + 2 {
                app.context_paste_requested = true;
            }
            return;
        }
    }
    let clamp = |point: (u16, u16), area: Option<ratatui::layout::Rect>| {
        area.filter(|area| area.width > 0 && area.height > 0)
            .map_or(point, |area| {
                (
                    point.0.clamp(area.y, area.bottom() - 1),
                    point.1.clamp(area.x, area.right() - 1),
                )
            })
    };
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let position = ratatui::layout::Position::new(mouse.column, mouse.row);
            app.screen_selection_area = app
                .chat_text_area
                .contains(position)
                .then_some(app.chat_text_area);
            app.chat_selection_scroll = app.chat_scroll;
            let point = clamp(point, app.screen_selection_area);
            app.screen_selection = Some((point, point));
            app.screen_selecting = true;
            app.draft_selection_anchor = None;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.screen_selecting => {
            if app.screen_selection_area.is_some() {
                app.follow_chat = false;
            }
            let point = clamp(point, app.screen_selection_area);
            app.screen_selection = app
                .screen_selection
                .map(|(start, _)| (start, (point.0, point.1.saturating_add(1))));
            return;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.screen_selecting {
                app.screen_selection = app.screen_selection.map(|(start, end)| {
                    if start == end {
                        (start, point)
                    } else {
                        (start, end)
                    }
                });
            }
            app.screen_selecting = false;
            return;
        }
        _ => (),
    }
    if app.terminal_overlay_open {
        return;
    }
    if !matches!(
        mouse.kind,
        MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    ) {
        return;
    }
    let position = ratatui::layout::Position::new(mouse.column, mouse.row);
    let target = app
        .click_targets
        .iter()
        .rev()
        .find(|(area, _)| area.contains(position))
        .map(|(_, target)| *target);
    if matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        if matches!(target, Some(ClickTarget::Pane(Focus::Chat))) {
            app.scroll_chat(mouse.kind == MouseEventKind::ScrollDown, 3);
        }
        if matches!(target, Some(ClickTarget::Pane(Focus::Workspace))) && app.workspace_tab == 1 {
            if app.diff_view.file_focus {
                app.diff_view
                    .move_file(&app.runs, mouse.kind == MouseEventKind::ScrollDown);
            } else {
                app.diff_view
                    .scroll(mouse.kind == MouseEventKind::ScrollDown, 3);
            }
        }
        if matches!(target, Some(ClickTarget::EditorEntry(_))) && app.workspace_tab == 0 {
            app.editor
                .move_tree(mouse.kind == MouseEventKind::ScrollDown, 3);
        }
        if matches!(target, Some(ClickTarget::DiffFile(_))) {
            app.diff_view
                .move_file(&app.runs, mouse.kind == MouseEventKind::ScrollDown);
        }
        if matches!(target, Some(ClickTarget::DiffBody)) {
            app.diff_view
                .scroll(mouse.kind == MouseEventKind::ScrollDown, 3);
        }
        if matches!(target, Some(ClickTarget::GitGraph)) {
            app.git
                .scroll_graph(mouse.kind == MouseEventKind::ScrollDown, 3);
        }
        return;
    }
    match target {
        Some(ClickTarget::RemoveAttachment(index)) => {
            if let Some(images) = app
                .draft_thread
                .as_ref()
                .and_then(|id| app.images.get_mut(id))
                && index < images.len()
            {
                images.remove(index);
                app.send_notice = Some("Attachment removed.".into());
            }
            app.focus = Focus::Composer;
        }
        Some(ClickTarget::SidebarRow(index)) => {
            if index < app.rows.len() {
                app.sidebar_state.select(Some(index));
                app.focus = Focus::Projects;
                app.editing = false;
                app.screen_selection = None;
                app.screen_selection_area = None;
                app.screen_selecting = false;
            }
        }
        Some(ClickTarget::StartupPrimary) => crate::startup::activate(app),
        Some(ClickTarget::StartupClose) => app.startup.open = false,
        Some(ClickTarget::QuitChoice(shutdown)) => app.confirm_quit(shutdown),
        Some(ClickTarget::QuitCancel) => app.quit_dialog = None,
        Some(ClickTarget::ThreadMenuOption(index)) => {
            if let Some(menu) = app.thread_menu.as_mut()
                && !menu.busy
            {
                menu.selected_option = index;
                menu.error = None;
                menu.mode = if index == 0 {
                    crate::app::ThreadMenuMode::Rename
                } else {
                    crate::app::ThreadMenuMode::ConfirmDelete
                };
            }
        }
        Some(ClickTarget::ThreadMenuConfirm) => confirm_thread_menu(app),
        Some(ClickTarget::SlashCommand(index)) => {
            app.apply_slash_command(index);
        }
        Some(ClickTarget::ThreadMenuCancel) => {
            if let Some(menu) = app.thread_menu.as_mut()
                && !menu.busy
            {
                menu.error = None;
                match menu.mode {
                    crate::app::ThreadMenuMode::Options
                    | crate::app::ThreadMenuMode::ProjectOptions => app.thread_menu = None,
                    crate::app::ThreadMenuMode::Rename
                    | crate::app::ThreadMenuMode::ConfirmDelete => {
                        menu.mode = crate::app::ThreadMenuMode::Options;
                    }
                }
            }
        }
        Some(ClickTarget::AutoStart(value)) => {
            app.settings_row = 1;
            app.auto_start_t3 = value;
            app.auto_start_highlight = value;
        }
        Some(ClickTarget::Telemetry(value)) => {
            app.settings_row = 4;
            app.telemetry_enabled = value;
            app.telemetry_highlight = value;
        }
        Some(ClickTarget::GitModeOption(index)) => app.git.select_mode(index),
        Some(ClickTarget::GitBranchOption(index)) => {
            app.git.branch_index = index;
            crate::git::key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        Some(ClickTarget::GitMessage) => {
            app.git.commit_row = 0;
            app.git.editing = true;
            app.git.message_follow_end = true;
        }
        Some(ClickTarget::GitFiles) => {
            app.git.commit_row = 1;
            app.git.editing = false;
        }
        Some(ClickTarget::GitControl(index)) => {
            app.editing = false;
            app.focus = Focus::Workspace;
            app.git.graph_focus = false;
            app.git.row = index;
            crate::git::key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        Some(ClickTarget::GitGraph) => {
            app.editing = false;
            app.focus = Focus::Workspace;
            app.workspace_tab = 3;
            app.git.graph_focus = true;
            if app
                .git
                .snapshot
                .as_ref()
                .and_then(|s| s.graph.get(app.git.graph_selected))
                .is_none_or(|line| !line.is_commit())
            {
                app.git.select_first_graph_commit();
            }
        }
        Some(ClickTarget::EditorEntry(index)) => {
            app.editing = false;
            app.focus = Focus::Workspace;
            app.workspace_tab = 0;
            app.workspace_highlight = 0;
            app.editor.tree_focus = true;
            app.editor.selected_entry = index;
            app.editor.activate_entry(index);
        }
        Some(ClickTarget::DiffBody) => {
            app.editing = false;
            app.focus = Focus::Workspace;
            app.diff_view.file_focus = false;
        }
        Some(ClickTarget::DiffFile(index)) => {
            app.editing = false;
            app.focus = Focus::Workspace;
            app.diff_view.select_file(index);
            app.diff_view.file_focus = true;
        }
        Some(ClickTarget::Pane(focus)) => {
            app.editing = false;
            app.focus = focus;
        }
        Some(ClickTarget::Workspace(index)) => {
            app.editing = false;
            app.focus = Focus::WorkspaceTabs;
            app.workspace_highlight = index;
            app.workspace_tab = index;
        }
        Some(ClickTarget::TypingStyle(style)) => {
            app.settings_row = 0;
            app.style_highlight = style;
            app.typing_style = style;
        }
        Some(ClickTarget::LayoutPreset(preset)) => {
            app.settings_row = 3;
            app.apply_layout_preset(preset);
        }
        Some(ClickTarget::ServerUpdateButton) => {
            app.settings_row = 5;
            app.trigger_server_update_action();
        }
        None => (),
    }
}

pub fn handle_key(app: &mut App, mut key: KeyEvent) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if app.terminal_overlay_open {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j') {
            app.terminal_overlay_open = false;
            app.local_terminal.error = None;
            if app
                .local_terminal
                .session
                .as_ref()
                .is_some_and(|s| s.status.lock().unwrap().is_some())
            {
                app.local_terminal.session = None;
            }
        } else if let Some(session) = &app.local_terminal.session {
            if key.modifiers.contains(KeyModifiers::SHIFT)
                && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
            {
                session.scroll(if key.code == KeyCode::PageUp { 10 } else { -10 });
            } else {
                session.scroll(-isize::MAX);
                let application = session.parser.lock().unwrap().screen().application_cursor();
                session.send(crate::local_terminal::key_bytes(key, application));
            }
        }
        return;
    }
    if app.focus == Focus::Workspace
        && app.workspace_tab == 2
        && app.local_terminal.tab_typing
        && app.quit_dialog.is_none()
        && !app.startup.open
        && !app.settings_open
        && !app.models.open
        && app.thread_menu.is_none()
    {
        if key.code == KeyCode::Esc {
            app.local_terminal.tab_typing = false;
        } else if let Some(session) = &app.local_terminal.session {
            if key.modifiers.contains(KeyModifiers::SHIFT)
                && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
            {
                session.scroll(if key.code == KeyCode::PageUp { 10 } else { -10 });
            } else {
                session.scroll(-isize::MAX);
                let application = session.parser.lock().unwrap().screen().application_cursor();
                session.send(crate::local_terminal::key_bytes(key, application));
            }
        }
        return;
    }
    let entering_text =
        if app.quit_dialog.is_some() || app.startup.open || app.settings_open || app.models.open {
            false
        } else if let Some(menu) = &app.thread_menu {
            menu.mode == crate::app::ThreadMenuMode::Rename
        } else {
            app.editing
                || (app.focus == Focus::Workspace && app.workspace_tab == 3 && app.git.editing)
                || (app.focus == Focus::Workspace
                    && app.workspace_tab == 0
                    && ((!app.editor.tree_focus && app.editor.open_file.is_some())
                        || (app.editor.tree_focus && app.editor.tree_has_input())))
                || (app.focus == Focus::Workspace
                    && app.workspace_tab == 2
                    && app.local_terminal.tab_typing)
        };
    if key.code == KeyCode::Char(' ')
        && key.modifiers.is_empty()
        && !entering_text
        && !app.terminal_overlay_open
    {
        key.code = KeyCode::Enter;
    }
    if app.context_menu.is_some() && key.code == KeyCode::Esc {
        app.context_menu = None;
        return;
    }
    if let Some(dialog) = app.quit_dialog.as_mut() {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q'))
        {
            app.confirm_quit(false);
            return;
        }
        match key.code {
            KeyCode::Esc => app.quit_dialog = None,
            KeyCode::Enter if key.kind != KeyEventKind::Repeat => {
                let shutdown = dialog.shutdown_server;
                app.confirm_quit(shutdown);
            }
            KeyCode::Char('y' | 'Y') => app.confirm_quit(true),
            KeyCode::Char('n' | 'N') => app.confirm_quit(false),
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('h' | 'l') => {
                dialog.shutdown_server = !dialog.shutdown_server;
            }
            _ => (),
        }
        return;
    }
    if app.thread_menu.is_some() {
        handle_thread_menu_key(app, key);
        return;
    }
    if !app.settings_open
        && !app.models.open
        && !app.startup.open
        && !app.terminal_overlay_open
        && key.code == KeyCode::Enter
        && key.kind != KeyEventKind::Repeat
        && !key.modifiers.contains(KeyModifiers::SHIFT)
        && (app.dictation.listening()
            || app.dictation.phase == crate::dictation::Phase::Transcribing)
    {
        if app.dictation.listening() {
            app.dictation.toggle_requested = true;
        }
        app.dictation.send_on_finish = true;
        app.editing = false;
        return;
    }
    if app.dictation.listening()
        && !app.editing
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
        && key.code == KeyCode::Esc
    {
        app.dictation.cancel_requested = true;
        return;
    }
    if key.code == KeyCode::Esc
        && app.focus == Focus::Composer
        && !app.editing
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
    {
        let now = std::time::Instant::now();
        if app.last_escape.is_some_and(|previous| {
            now.duration_since(previous) <= std::time::Duration::from_millis(500)
        }) {
            app.clear_draft();
            app.send_notice = None;
            app.last_escape = None;
        } else {
            app.last_escape = Some(now);
        }
        return;
    }
    if key.code != KeyCode::Esc {
        app.last_escape = None;
    }
    if app.focus == Focus::Workspace
        && app.workspace_tab == 0
        && ((!app.editor.tree_focus && app.editor.open_file.is_some())
            || (app.editor.tree_focus && app.editor.tree_has_input()))
        && app.quit_dialog.is_none()
        && !app.startup.open
        && !app.settings_open
        && !app.models.open
    {
        crate::editor::key(app, key);
        return;
    }
    if app.focus == Focus::Composer
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
        && !app.access_open
        && !app.git.has_modal()
        && app.thread_menu.is_none()
        && app.active_approval_popup().is_none()
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('z' | 'Z' | 'y' | 'Y'))
    {
        let redo = matches!(key.code, KeyCode::Char('y' | 'Y' | 'Z'))
            || key.modifiers.contains(KeyModifiers::SHIFT);
        app.undo_draft(redo);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('c')
        && app.focus == Focus::Composer
        && !app.terminal_overlay_open
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
    {
        if app.selected_draft().is_some() || app.selected_screen().is_some() {
            copy_selection(app);
        }
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('c')
        && (app.selected_draft().is_some() || app.selected_screen().is_some())
    {
        copy_selection(app);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j') {
        app.terminal_overlay_open = !app.terminal_overlay_open;
        return;
    }
    if matches!(key.code, KeyCode::Char('T') | KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('T'))
        && (app.terminal_overlay_open
            || (!app.editing
                && !app.git.editing
                && !app.settings_open
                && !app.models.open
                && !app.startup.open))
    {
        app.terminal_overlay_open = !app.terminal_overlay_open;
        return;
    }
    if app.git.has_modal()
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
        && !app.access_open
        && app.active_approval_popup().is_none()
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Enter | KeyCode::Char('g'))
    {
        crate::git::key(app, key);
        return;
    }
    if app.editing
        && !app.settings_open
        && !app.models.open
        && !app.startup.open
        && key.code == KeyCode::Enter
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT))
    {
        app.steer_requested = true;
        app.send_requested = true;
        app.editing = false;
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(key.code, KeyCode::Char('c' | 'q')) {
            app.request_quit();
        } else if app.editing
            && !app.settings_open
            && !app.models.open
            && !app.startup.open
            && matches!(key.code, KeyCode::Backspace | KeyCode::Char('w'))
        {
            app.backspace_draft_word();
        } else if app.editing
            && !app.settings_open
            && !app.models.open
            && !app.startup.open
            && key.code == KeyCode::Char('a')
        {
            app.select_all_draft();
        } else if matches!(key.code, KeyCode::Backspace | KeyCode::Char('h'))
            && matches!(app.focus, Focus::Composer | Focus::Chat)
            && !app.settings_open
            && !app.models.open
            && !app.startup.open
        {
            let removed_images = app
                .draft_thread
                .as_ref()
                .and_then(|thread| app.images.remove(thread))
                .is_some_and(|images| !images.is_empty());
            let canceled_paste = app.paste_pending || app.paste_requested;
            if removed_images || canceled_paste {
                app.paste_generation = app.paste_generation.wrapping_add(1);
                app.paste_pending = false;
                app.paste_requested = false;
                app.send_notice = Some("Attached images removed.".into());
            }
        } else if key.code == KeyCode::Char('v')
            && matches!(app.focus, Focus::Composer)
            && !app.settings_open
            && !app.models.open
            && !app.startup.open
        {
            if !app.paste_pending {
                app.context_paste_requested = true;
            }
        } else if app.editing && !app.settings_open && !app.models.open {
            match key.code {
                KeyCode::Left | KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    app.draft_selection_anchor.get_or_insert(app.draft_cursor);
                    app.screen_selection = None;
                    app.move_draft_word(key.code == KeyCode::Right);
                }
                KeyCode::Left => {
                    app.draft_selection_anchor = None;
                    app.move_draft_word(false);
                }
                KeyCode::Right => {
                    app.draft_selection_anchor = None;
                    app.move_draft_word(true);
                }
                _ => (),
            }
        }
        return;
    }
    if app.startup.open {
        match key.code {
            KeyCode::Enter if key.kind != KeyEventKind::Repeat => crate::startup::activate(app),
            KeyCode::Esc => app.startup.open = false,
            KeyCode::Char('q') => app.request_quit(),
            KeyCode::Char('s') => {
                app.startup.open = false;
                app.settings_open = true;
                app.clipboard_check_requested = true;
                app.auto_start_highlight = app.auto_start_t3;
                app.telemetry_highlight = app.telemetry_enabled;
                app.style_highlight = app.typing_style;
                app.request_server_version_check();
            }
            _ => (),
        }
        return;
    }
    if app.settings_open {
        match key.code {
            KeyCode::Char('r') => {
                app.clipboard_status = crate::app::ClipboardStatus::Checking;
                app.clipboard_check_requested = true;
                app.request_server_version_check();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.settings_row = app.settings_row.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => app.settings_row = (app.settings_row + 1).min(5),
            KeyCode::Right | KeyCode::Char('l') if app.settings_row == 1 => {
                app.auto_start_highlight = false
            }
            KeyCode::Left | KeyCode::Char('h') if app.settings_row == 1 => {
                app.auto_start_highlight = true
            }
            KeyCode::Enter | KeyCode::Char(' ') if app.settings_row == 1 => {
                app.auto_start_t3 = app.auto_start_highlight
            }
            KeyCode::Left | KeyCode::Char('h') if app.settings_row == 2 => {
                app.diff_view.wrap = false
            }
            KeyCode::Right | KeyCode::Char('l') if app.settings_row == 2 => {
                app.diff_view.wrap = true
            }
            KeyCode::Enter | KeyCode::Char(' ') if app.settings_row == 2 => {
                app.diff_view.wrap = app.diff_wrap_highlight
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
                if app.settings_row == 3 =>
            {
                app.layout_highlight = app
                    .layout_highlight
                    .cycle(matches!(key.code, KeyCode::Right | KeyCode::Char('l')));
            }
            KeyCode::Enter | KeyCode::Char(' ') if app.settings_row == 3 => {
                app.apply_layout_preset(app.layout_highlight);
            }
            KeyCode::Right | KeyCode::Char('l') if app.settings_row == 4 => {
                app.telemetry_highlight = false
            }
            KeyCode::Left | KeyCode::Char('h') if app.settings_row == 4 => {
                app.telemetry_highlight = true
            }
            KeyCode::Enter | KeyCode::Char(' ') if app.settings_row == 4 => {
                app.telemetry_enabled = app.telemetry_highlight
            }
            KeyCode::Enter | KeyCode::Char(' ') if app.settings_row == 5 => {
                app.trigger_server_update_action();
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
                if app.settings_row == 5 => {}
            KeyCode::Esc | KeyCode::Char('s') => app.settings_open = false,
            KeyCode::Right | KeyCode::Char('l') => {
                app.style_highlight = app.style_highlight.cycle(true);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.style_highlight = app.style_highlight.cycle(false)
            }
            KeyCode::Enter | KeyCode::Char(' ') => app.typing_style = app.style_highlight,
            _ => (),
        }
        return;
    }
    if app.models.open {
        match key.code {
            KeyCode::Esc => app.models.open = false,
            KeyCode::Enter if !app.models.reasoning_focus => app.models.reasoning_focus = true,
            KeyCode::Enter => app.models.change_requested = true,
            KeyCode::Down | KeyCode::Char('j') if app.models.reasoning_focus => {
                app.move_reasoning(true)
            }
            KeyCode::Down | KeyCode::Char('j') => app.move_model(true, 1),
            KeyCode::Up | KeyCode::Char('k') if app.models.reasoning_focus => {
                app.move_reasoning(false)
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_model(false, 1),
            KeyCode::Right | KeyCode::Char('l') => app.models.reasoning_focus = true,
            KeyCode::Left | KeyCode::Char('h') => app.models.reasoning_focus = false,
            KeyCode::Tab => app.models.reasoning_focus = !app.models.reasoning_focus,
            KeyCode::PageDown => app.move_model(true, 10),
            KeyCode::PageUp => app.move_model(false, 10),
            KeyCode::Home => app.move_model(false, usize::MAX),
            KeyCode::End => app.move_model(true, usize::MAX),
            KeyCode::Char('r') if !app.models.loading => app.request_models(),
            _ => (),
        }
        return;
    }
    if app.access_open {
        let count = crate::app::RUNTIME_MODES.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.access_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                app.access_highlight = app.access_highlight.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.access_highlight = (app.access_highlight + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Char(' ') if key.kind != KeyEventKind::Repeat => {
                app.confirm_access_picker();
            }
            _ => (),
        }
        return;
    }
    if let Some(pending) = app.active_approval_popup() {
        let count = pending.options.len();
        match key.code {
            KeyCode::Esc => {
                app.approval_dismissed_id = Some(pending.request_id);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.approval_highlight = app.approval_highlight.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.approval_highlight = (app.approval_highlight + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Char(' ') if key.kind != KeyEventKind::Repeat => {
                app.confirm_approval();
            }
            KeyCode::Char(ch @ '1'..='5') if key.kind != KeyEventKind::Repeat => {
                let idx = (ch as usize) - ('1' as usize);
                if idx < count {
                    app.approval_highlight = idx;
                    app.confirm_approval();
                }
            }
            _ => (),
        }
        return;
    }
    let git_modal = app.git.has_modal() || app.git.editing;
    if git_modal && crate::git::key(app, key) {
        return;
    }
    if app.focus == Focus::Workspace && app.workspace_tab == 3 && crate::git::key(app, key) {
        return;
    }
    if app.focus == Focus::Workspace && app.workspace_tab == 0 && crate::editor::key(app, key) {
        return;
    }
    if app.focus == Focus::Workspace && app.workspace_tab == 2 {
        match key.code {
            KeyCode::Enter | KeyCode::Char('i') => {
                app.local_terminal.tab_typing = true;
                return;
            }
            KeyCode::PageUp => {
                if let Some(session) = &app.local_terminal.session {
                    session.scroll(10);
                }
                return;
            }
            KeyCode::PageDown => {
                if let Some(session) = &app.local_terminal.session {
                    session.scroll(-10);
                }
                return;
            }
            _ => (),
        }
    }
    if !app.editing
        && matches!(key.code, KeyCode::Char('D') | KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('D'))
    {
        app.dictation.toggle_requested = true;
        return;
    }
    if !app.editing
        && let KeyCode::Char(mut ch) = key.code
    {
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            ch = ch.to_ascii_uppercase();
        }
        if let Some(index) = match ch {
            'e' | 'E' => Some(0),
            'd' => Some(1),
            't' => Some(2),
            'g' => Some(3),
            _ => None,
        } {
            app.workspace_tab = index;
            app.workspace_highlight = index;
            app.focus = Focus::Workspace;
            return;
        }
    }
    if app.editing {
        let slash_matches = app.matching_slash_commands();
        if !slash_matches.is_empty() {
            match key.code {
                KeyCode::Up if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    app.slash_highlight = app.slash_highlight.saturating_sub(1);
                    return;
                }
                KeyCode::Down if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    app.slash_highlight = (app.slash_highlight + 1).min(slash_matches.len() - 1);
                    return;
                }
                KeyCode::Tab => {
                    let index = app.slash_highlight.min(slash_matches.len() - 1);
                    app.apply_slash_command(index);
                    return;
                }
                KeyCode::Enter
                    if !key.modifiers.contains(KeyModifiers::SHIFT)
                        && key.kind != KeyEventKind::Repeat =>
                {
                    let index = app.slash_highlight.min(slash_matches.len() - 1);
                    app.apply_slash_command(index);
                    return;
                }
                KeyCode::Esc => {
                    app.slash_dismissed = true;
                    return;
                }
                _ => (),
            }
        }
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => app.insert_draft("\n"),
            KeyCode::Enter => {
                app.send_requested = true;
                app.editing = false;
            }
            KeyCode::Esc => app.editing = false,
            KeyCode::Char(character) => {
                if character == '/' {
                    app.slash_dismissed = false;
                    app.slash_highlight = 0;
                }
                app.insert_draft(&character.to_string());
                if app.slash_query().is_none() {
                    app.slash_dismissed = false;
                    app.slash_highlight = 0;
                } else {
                    let len = app.matching_slash_commands().len();
                    app.slash_highlight = app.slash_highlight.min(len.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                app.backspace_draft();
                if app.slash_query().is_none() {
                    app.slash_dismissed = false;
                    app.slash_highlight = 0;
                } else {
                    let len = app.matching_slash_commands().len();
                    app.slash_highlight = app.slash_highlight.min(len.saturating_sub(1));
                }
            }
            KeyCode::Delete => app.delete_draft(),
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    app.draft_selection_anchor.get_or_insert(app.draft_cursor);
                } else {
                    app.draft_selection_anchor = None;
                }
                app.screen_selection = None;
                app.move_draft_cursor(key.code);
            }
            _ => (),
        }
        return;
    }
    if app.focus == Focus::Workspace && app.workspace_tab == 1 {
        if app.diff_view.file_focus {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    app.diff_view.move_file(&app.runs, true);
                    return;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.diff_view.file == 0 {
                        app.focus = Focus::WorkspaceTabs;
                    } else {
                        app.diff_view.move_file(&app.runs, false);
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                    app.diff_view.file_focus = false;
                    return;
                }
                _ => (),
            }
        }
        match key.code {
            KeyCode::Right | KeyCode::Char('l')
                if !app.diff_view.file_focus && !key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                app.diff_view.file_focus = true;
                return;
            }
            KeyCode::Esc => {
                app.diff_view.file_focus = true;
                return;
            }
            KeyCode::PageDown => {
                app.diff_view.scroll(true, 10);
                return;
            }
            KeyCode::PageUp => {
                app.diff_view.scroll(false, 10);
                return;
            }
            KeyCode::Home => {
                app.diff_view.scroll = 0;
                return;
            }
            KeyCode::End => {
                app.diff_view.scroll = app.diff_view.max_scroll;
                return;
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.diff_view.scroll(true, 1);
                return;
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.diff_view.scroll(false, 1);
                return;
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.diff_view.horizontal = app.diff_view.horizontal.saturating_add(4);
                return;
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.diff_view.horizontal = app.diff_view.horizontal.saturating_sub(4);
                return;
            }
            KeyCode::Char('[' | ']') => {
                app.diff_view
                    .select_run(&app.runs, key.code == KeyCode::Char(']'));
                return;
            }
            KeyCode::Char('r') => {
                if let Some(id) = app.diff_view.run_id(&app.runs).map(str::to_owned) {
                    app.diff_requested.remove(&id);
                    app.diff_view.errors.remove(&id);
                    app.diff_view.patches.remove(&id);
                }
                return;
            }
            _ => (),
        }
    }
    match key.code {
        KeyCode::Char('w') if !app.editing => {
            app.workspace_visible = !app.workspace_visible;
            if !app.workspace_visible
                && matches!(app.focus, Focus::Workspace | Focus::WorkspaceTabs)
            {
                app.focus = Focus::Chat;
            }
        }
        KeyCode::Char('}') if !app.editing => {
            app.workspace_width = (app.workspace_width + 5).min(55)
        }
        KeyCode::Char('{') if !app.editing => {
            app.workspace_width = app.workspace_width.saturating_sub(5).max(20)
        }
        KeyCode::Char('r') if !app.editing && app.focus == Focus::Chat => {
            app.retry_previous_message();
        }
        KeyCode::Char('s') => {
            app.settings_row = 0;
            app.auto_start_highlight = app.auto_start_t3;
            app.telemetry_highlight = app.telemetry_enabled;
            app.style_highlight = app.typing_style;
            app.layout_highlight = app.layout_preset;
            app.settings_open = true;
            app.clipboard_check_requested = true;
            app.request_server_version_check();
        }
        KeyCode::Char('o') => app.startup.open = true,
        KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => app.focus = Focus::Chat,
        KeyCode::Char('m') => app.focus = Focus::Composer,
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Chat => {
            app.focus = Focus::Workspace;
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Model => {
            app.focus = Focus::Access;
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::Workspace => {
            app.focus = Focus::WorkspaceTabs;
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::WorkspaceTabs => {
            app.focus = Focus::Workspace;
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::WorkspaceTabs => {
            app.workspace_highlight = (app.workspace_highlight + 1) % 4;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::WorkspaceTabs => {
            if app.workspace_highlight == 0 {
                app.focus = Focus::Chat;
            } else {
                app.workspace_highlight -= 1;
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::WorkspaceTabs => {
            app.workspace_tab = app.workspace_highlight;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Workspace => {
            app.focus = Focus::Chat;
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k')
            if matches!(app.focus, Focus::Workspace | Focus::WorkspaceTabs) => {}
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::Model => app.open_models(),
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::Access => {
            app.open_access_picker()
        }
        KeyCode::Enter | KeyCode::Char('a')
            if app.focus == Focus::Chat && app.pending_approval().is_some() =>
        {
            app.approval_dismissed_id = None;
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Composer => {
            app.focus = Focus::Model
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::Model => {
            app.focus = Focus::Composer
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Model => {
            app.focus = Focus::SessionUsage
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Access => {
            app.focus = Focus::Model
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Access => {
            app.focus = Focus::SessionUsage
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::Access => {
            app.focus = Focus::Composer
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Access => {
            app.focus = Focus::SessionUsage
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::SessionUsage => {
            app.focus = Focus::Access
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::SessionUsage => {
            app.focus = Focus::WeeklyUsage
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::SessionUsage => {
            app.focus = Focus::Composer
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::WeeklyUsage => {
            app.focus = Focus::SessionUsage
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::WeeklyUsage => {
            app.focus = Focus::Workspace
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::WeeklyUsage => {
            app.focus = Focus::Composer
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Composer => {
            app.focus = Focus::Workspace
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Model => {
            app.focus = Focus::Composer
        }
        KeyCode::PageDown if app.focus == Focus::Chat => app.scroll_chat(true, 10),
        KeyCode::PageUp if app.focus == Focus::Chat => app.scroll_chat(false, 10),
        KeyCode::Home if app.focus == Focus::Chat => {
            app.chat_scroll = 0;
            app.follow_chat = false;
        }
        KeyCode::End if app.focus == Focus::Chat => {
            app.chat_scroll = app.chat_max_scroll;
            app.follow_chat = true;
        }
        KeyCode::Down
            if app.focus == Focus::Chat && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.scroll_chat(true, 1)
        }
        KeyCode::Char('j' | 'J')
            if app.focus == Focus::Chat && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.scroll_chat(true, 1)
        }
        KeyCode::Up if app.focus == Focus::Chat && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_chat(false, 1)
        }
        KeyCode::Char('k' | 'K')
            if app.focus == Focus::Chat && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.scroll_chat(false, 1)
        }
        KeyCode::Char('p') if !app.authenticated && !app.pairing_in_progress => {
            app.pairing_requested = true;
        }
        KeyCode::Char('p') if key.modifiers == KeyModifiers::NONE => {
            app.focus = Focus::Projects;
        }
        KeyCode::Left | KeyCode::Char('h') => app.focus = Focus::Projects,
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Projects => {
            app.focus = Focus::Chat;
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Projects => {
            app.move_selection(true)
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::Projects => {
            app.move_selection(false)
        }
        KeyCode::Enter if app.focus == Focus::Projects => {
            app.open_thread_menu();
        }
        KeyCode::Down | KeyCode::Char('j') => app.focus = Focus::Composer,
        KeyCode::Up | KeyCode::Char('k') => app.focus = Focus::Chat,
        KeyCode::Enter | KeyCode::Char('i') if app.focus == Focus::Composer => {
            app.editing = true;
        }
        KeyCode::Char('q') => app.request_quit(),
        _ => (),
    }
}

fn confirm_thread_menu(app: &mut App) {
    let Some(menu) = app.thread_menu.as_mut() else {
        return;
    };
    if menu.busy {
        return;
    }
    match menu.mode {
        crate::app::ThreadMenuMode::ProjectOptions => {
            let project_id = menu.thread_id.clone();
            let model = app
                .catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog
                        .threads
                        .iter()
                        .find(|t| t.project_id == project_id && t.model_selection.is_some())
                })
                .and_then(|t| t.model_selection.clone());
            menu.busy = true;
            app.thread_action_requested = Some(crate::t3::ThreadAction::Create {
                project_id,
                thread_id: uuid::Uuid::new_v4().to_string(),
                model,
            });
        }
        crate::app::ThreadMenuMode::Options => {
            menu.error = None;
            menu.mode = if menu.selected_option == 0 {
                crate::app::ThreadMenuMode::Rename
            } else {
                crate::app::ThreadMenuMode::ConfirmDelete
            };
        }
        crate::app::ThreadMenuMode::Rename => {
            let trimmed = menu.rename_input.trim().to_owned();
            if trimmed.is_empty() {
                menu.error = Some("Thread title cannot be empty.".into());
                return;
            }
            menu.busy = true;
            menu.error = None;
            app.thread_action_requested = Some(crate::t3::ThreadAction::Rename {
                thread_id: menu.thread_id.clone(),
                title: trimmed,
            });
        }
        crate::app::ThreadMenuMode::ConfirmDelete => {
            menu.busy = true;
            menu.error = None;
            app.thread_action_requested = Some(crate::t3::ThreadAction::Delete {
                thread_id: menu.thread_id.clone(),
            });
        }
    }
}

fn handle_thread_menu_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'q'))
    {
        app.thread_menu = None;
        app.request_quit();
        return;
    }
    let Some(menu) = app.thread_menu.as_mut() else {
        return;
    };
    if menu.busy {
        return;
    }
    match menu.mode {
        crate::app::ThreadMenuMode::ProjectOptions => match key.code {
            KeyCode::Esc => app.thread_menu = None,
            KeyCode::Enter | KeyCode::Char('n') if key.kind != KeyEventKind::Repeat => {
                confirm_thread_menu(app)
            }
            _ => (),
        },
        crate::app::ThreadMenuMode::Options => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.thread_menu = None,
            KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Tab => {
                menu.selected_option = 1 - menu.selected_option;
            }
            KeyCode::Char('r') => {
                menu.selected_option = 0;
                menu.error = None;
                menu.mode = crate::app::ThreadMenuMode::Rename;
            }
            KeyCode::Char('d') => {
                menu.selected_option = 1;
                menu.error = None;
                menu.mode = crate::app::ThreadMenuMode::ConfirmDelete;
            }
            KeyCode::Enter if key.kind != KeyEventKind::Repeat => confirm_thread_menu(app),
            _ => (),
        },
        crate::app::ThreadMenuMode::Rename => match key.code {
            KeyCode::Esc => {
                menu.error = None;
                menu.mode = crate::app::ThreadMenuMode::Options;
            }
            KeyCode::Enter if key.kind != KeyEventKind::Repeat => confirm_thread_menu(app),
            KeyCode::Backspace => {
                menu.rename_input.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                menu.rename_input.push(c);
            }
            _ => (),
        },
        crate::app::ThreadMenuMode::ConfirmDelete => match key.code {
            KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                menu.error = None;
                menu.mode = crate::app::ThreadMenuMode::Options;
            }
            KeyCode::Enter if key.kind != KeyEventKind::Repeat => confirm_thread_menu(app),
            KeyCode::Char('y' | 'Y') => confirm_thread_menu(app),
            _ => (),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_undo_redo_shortcuts_restore_edits_without_typing_control_characters() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..Default::default()
        };
        app.insert_draft("A pasted message");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert!(app.draft.is_empty());
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft, "A pasted message");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        handle_key(
            &mut app,
            KeyEvent::new(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert_eq!(app.draft, "A pasted message");
        app.settings_open = true;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft, "A pasted message");
    }

    #[test]
    fn space_activates_menus_and_preserves_spaces_in_text_fields() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(0));
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        handle_key(&mut app, space);
        assert_eq!(
            app.thread_menu.as_ref().unwrap().mode,
            crate::app::ThreadMenuMode::ProjectOptions
        );
        handle_key(&mut app, space);
        assert!(matches!(
            app.thread_action_requested,
            Some(crate::t3::ThreadAction::Create { .. })
        ));
        app.thread_menu = None;
        app.focus = Focus::Composer;
        handle_key(&mut app, space);
        assert!(app.editing);
        assert!(app.draft.is_empty());
        handle_key(&mut app, space);
        assert_eq!(app.draft, " ");
        assert!(!app.send_requested);
        app.editing = false;
        app.open_thread_menu();
        let menu = app.thread_menu.as_mut().unwrap();
        menu.mode = crate::app::ThreadMenuMode::Rename;
        menu.rename_input = "Two".into();
        handle_key(&mut app, space);
        assert_eq!(app.thread_menu.as_ref().unwrap().rename_input, "Two ");
    }

    #[test]
    fn project_enter_menu_creates_a_thread_in_that_project() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(0));
        let Some(crate::catalog::Selection::Project(project_id)) = app.selection().cloned() else {
            panic!("expected project");
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.thread_menu.as_ref().unwrap().mode,
            crate::app::ThreadMenuMode::ProjectOptions
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(&app.thread_action_requested, Some(crate::t3::ThreadAction::Create { project_id: id, .. }) if *id == project_id)
        );
        assert!(app.thread_menu.as_ref().unwrap().busy);
    }

    #[test]
    fn clear_attachments_handles_terminal_control_h_and_invalidates_pending_paste() {
        for code in [KeyCode::Backspace, KeyCode::Char('h')] {
            let mut app = App::default();
            app.focus = Focus::Chat;
            app.draft_thread = Some("thread".into());
            app.draft = "Keep text".into();
            handle_key(&mut app, KeyEvent::new(code, KeyModifiers::CONTROL));
            assert_eq!(app.send_notice, None);

            app.images
                .insert("thread".into(), vec![serde_json::json!({"id":"image"})]);
            app.paste_pending = true;
            handle_key(&mut app, KeyEvent::new(code, KeyModifiers::CONTROL));
            assert!(!app.images.contains_key("thread"));
            assert!(!app.paste_pending);
            assert_eq!(app.paste_generation, 1);
            assert_eq!(app.draft, "Keep text");
            assert_eq!(app.send_notice.as_deref(), Some("Attached images removed."));
        }
    }

    #[test]
    fn panel_hotkeys_focus_projects_and_chat_without_intercepting_ctrl_c() {
        let mut app = App {
            authenticated: true,
            focus: Focus::Composer,
            ..App::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        );
        assert!(app.focus == Focus::Projects);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        assert!(app.focus == Focus::Chat);

        app.focus = Focus::Projects;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(app.focus == Focus::Projects);
        assert!(app.quit);
    }

    #[test]
    fn ctrl_enter_send_keeps_focus_on_composer_outside_typing_mode() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        );
        assert!(app.send_requested);
        assert!(app.steer_requested);
        assert!(!app.editing);
        assert!(app.focus == Focus::Composer);
    }

    #[test]
    fn access_level_picker_updates_runtime_mode() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.sync_history_selection();
        app.focus = Focus::Model;

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );
        assert!(app.focus == Focus::Access);

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.access_open);

        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.access_open);
        let req = app.access_requested.take().unwrap();
        assert_eq!(req.thread_id, "t1");
        assert_eq!(req.runtime_mode, "auto-accept-edits");
        assert_eq!(req.payload()["type"], "thread.runtime-mode.set");

        app.apply_runtime_mode_change(req, Ok(()));
        assert_eq!(app.current_runtime_mode(), Some("auto-accept-edits"));
    }

    #[test]
    fn approval_popup_responds_and_can_be_minimized() {
        let mut app = App::default();
        app.authenticated = true;
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.sync_history_selection();
        app.history_thread = Some("t1".into());
        app.transcript = Some(
            serde_json::from_value(serde_json::json!({
                "threadId": "t1",
                "title": "Chat",
                "items": [{
                    "position": 0,
                    "item": {
                        "type": "approval_request",
                        "requestId": "req-1",
                        "requestKind": "command",
                        "prompt": "Run cargo test"
                    }
                }]
            }))
            .unwrap(),
        );
        assert!(app.active_approval_popup().is_some());

        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.active_approval_popup().is_none());

        app.focus = Focus::Chat;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        );
        assert!(app.active_approval_popup().is_some());

        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let req = app.approval_requested.take().unwrap();
        assert_eq!(req.request_id, "req-1");
        assert_eq!(req.decision, "acceptForSession");
        assert_eq!(req.payload()["type"], "runtime-request.respond");
    }

    #[test]
    fn workspace_hotkeys_are_case_sensitive_and_do_not_intercept_text_inputs() {
        let mut app = App::default();
        for (ch, index) in [('e', 0), ('E', 0), ('d', 1), ('g', 3), ('t', 2)] {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
            assert_eq!(app.workspace_tab, index);
            assert_eq!(app.workspace_highlight, index);
            assert!(app.focus == Focus::Workspace);
        }
        app.focus = Focus::Composer;
        app.editing = true;
        for ch in "DCftgc".chars() {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        assert_eq!(app.draft, "DCftgc");
        assert!(app.focus == Focus::Composer);
        app.editing = false;
        app.focus = Focus::Workspace;
        app.workspace_tab = 3;
        app.git.editing = true;
        for ch in "DCftgc".chars() {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        assert_eq!(app.git.message, "DCftgc");
        assert_eq!(app.workspace_tab, 3);
    }

    #[test]
    fn setup_requires_explicit_install_and_settings_confirm_auto_start_changes() {
        let mut app = App::default();
        app.startup.open = true;
        app.startup.event = crate::startup::Event {
            message: "Missing".into(),
            detail: String::new(),
            offer: crate::startup::Offer::Install,
            busy: false,
            local_server_pid: None,
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.startup.request.is_none());
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(!app.auto_start_highlight);
        assert!(app.auto_start_t3);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.auto_start_t3);
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.auto_start_highlight);
        app.settings_open = false;
        app.startup.open = true;
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            app.startup.request,
            Some(crate::startup::Command::Install)
        ));
    }

    #[test]
    fn arrows_move_between_diff_and_persistent_file_list() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 1,
            ..Default::default()
        };
        app.diff_view.file_focus = true;
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!app.diff_view.file_focus);
        assert!(app.focus == Focus::Workspace);
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(app.diff_view.file_focus);
        assert!(app.focus == Focus::Workspace);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );
        assert!(app.diff_view.file_focus);
    }

    #[test]
    fn control_arrows_jump_words_while_typing_without_changing_the_draft() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..Default::default()
        };
        app.insert_draft("héllo, rust\nworld");
        for expected in [12, 7, 0, 0] {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
            );
            assert_eq!(app.draft_cursor, expected);
        }
        for expected in [7, 12, 17, 17] {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
            );
            assert_eq!(app.draft_cursor, expected);
        }
        assert_eq!(app.draft, "héllo, rust\nworld");
        assert!(app.focus == Focus::Composer && app.editing && !app.quit);
        app.editing = false;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft_cursor, 17);
    }

    #[test]
    fn deleting_unicode_graphemes_and_stale_cursors_cannot_leave_draft_text_stuck() {
        let mut app = App::default();
        app.insert_draft("e\u{301}👩‍💻");
        app.backspace_draft();
        assert_eq!(app.draft, "e\u{301}");
        app.backspace_draft();
        assert!(app.draft.is_empty());
        app.insert_draft("abc");
        app.draft_cursor = 999;
        app.backspace_draft();
        assert_eq!(app.draft, "ab");
        assert_eq!(app.draft_cursor, 2);
        app.insert_draft("\r\nnext\tline");
        assert_eq!(app.draft, "ab\nnext    line");
    }

    #[test]
    fn wheel_over_chat_scrolls_without_interrupting_typing_and_resumes_follow_at_bottom() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            chat_scroll: 10,
            chat_max_scroll: 10,
            ..Default::default()
        };
        app.click_targets.push((
            ratatui::layout::Rect::new(10, 1, 40, 20),
            crate::app::ClickTarget::Pane(Focus::Chat),
        ));
        let mut wheel = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 15,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, wheel);
        assert_eq!(app.chat_scroll, 7);
        assert!(!app.follow_chat);
        assert!(app.editing && app.focus == Focus::Composer);
        wheel.kind = MouseEventKind::ScrollDown;
        handle_mouse(&mut app, wheel);
        assert_eq!(app.chat_scroll, 10);
        assert!(app.follow_chat);
        wheel.kind = MouseEventKind::ScrollUp;
        wheel.column = 0;
        handle_mouse(&mut app, wheel);
        assert_eq!(app.chat_scroll, 10);
    }

    #[test]
    fn workspace_selector_switches_tools_without_editing_the_prompt() {
        let mut app = App {
            focus: Focus::Chat,
            ..App::default()
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(app.focus == Focus::Workspace);
        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(app.focus == Focus::WorkspaceTabs);
        for index in 1..=4 {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
            );
            assert_eq!(app.workspace_highlight, index % 4);
            assert_eq!(app.workspace_tab, 0);
        }
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.focus == Focus::Chat);
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        for _ in 0..3 {
            handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        assert_eq!(app.workspace_highlight, 3);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.workspace_tab, 3);
        assert!(!app.editing);
        assert!(app.draft.is_empty());
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.focus == Focus::Chat);
    }

    #[test]
    fn settings_style_changes_only_when_confirmed() {
        let mut app = App::default();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.typing_style, crate::preferences::TypingStyle::Regular);
        assert_eq!(app.style_highlight, crate::preferences::TypingStyle::Vim);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.typing_style, crate::preferences::TypingStyle::Vim);
    }

    #[test]
    fn settings_r_requests_a_clipboard_recheck() {
        let mut app = App::default();
        app.settings_open = true;
        app.clipboard_check_requested = false;
        app.clipboard_status = crate::app::ClipboardStatus::Unavailable("old failure".into());
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        );
        assert!(app.clipboard_check_requested);
        assert!(matches!(
            app.clipboard_status,
            crate::app::ClipboardStatus::Checking
        ));
    }

    #[test]
    fn model_box_is_focusable_and_popup_keys_do_not_edit_or_send_prompt() {
        let mut app = App {
            focus: Focus::Composer,
            authenticated: true,
            ..Default::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.models.choices = crate::models::choices(crate::models::tests::catalog()).unwrap();
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.focus == Focus::Model);
        assert!(!app.editing);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.models.open);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(app.models.list_state.selected(), Some(1));
        assert!(app.draft.is_empty());
        assert!(!app.send_requested);
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.models.open);
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.focus == Focus::Composer);
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(app.focus == Focus::Composer);
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.focus == Focus::Model);
    }

    #[test]
    fn down_or_j_highlights_composer_without_entering_typing_mode() {
        for code in [KeyCode::Down, KeyCode::Char('j')] {
            let mut app = App {
                focus: Focus::Chat,
                ..Default::default()
            };
            handle_key(&mut app, KeyEvent::new(code, KeyModifiers::NONE));
            assert!(app.focus == Focus::Composer);
            assert!(!app.editing);
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            );
            assert!(app.draft.is_empty());
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
            );
            assert!(app.editing);
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            assert!(app.focus == Focus::Chat);
        }
    }

    #[test]
    fn shift_arrows_scroll_chat_without_switching_panes() {
        let mut app = App {
            focus: Focus::Chat,
            chat_scroll: 5,
            chat_max_scroll: 20,
            ..Default::default()
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(app.chat_scroll, 6);
        assert!(app.focus == Focus::Chat);
    }

    #[test]
    fn shift_jk_scroll_chat_without_switching_panes() {
        for (code, expected) in [(KeyCode::Char('J'), 6), (KeyCode::Char('K'), 4)] {
            let mut app = App {
                focus: Focus::Chat,
                chat_scroll: 5,
                chat_max_scroll: 20,
                ..Default::default()
            };
            handle_key(&mut app, KeyEvent::new(code, KeyModifiers::SHIFT));
            assert_eq!(app.chat_scroll, expected);
            assert!(app.focus == Focus::Chat);
            assert!(!app.follow_chat);
        }
    }

    #[test]
    fn enter_sends_only_while_typing_and_shift_enter_adds_a_newline() {
        let mut app = App {
            focus: Focus::Composer,
            ..App::default()
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.editing);
        assert!(!app.send_requested);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(app.draft, "\n");
        assert!(!app.send_requested);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.send_requested);
        assert!(!app.editing);
        assert!(app.focus == Focus::Composer);
    }

    #[test]
    fn shift_arrows_select_draft_and_plain_arrow_clears_selection() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        app.insert_draft("abc");
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert_eq!(app.selected_draft().as_deref(), Some("bc"));
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(app.selected_draft().is_none());
    }

    #[test]
    fn ctrl_c_copies_selected_message_text_instead_of_clearing_it() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        app.insert_draft("draft message");
        app.draft_selection_anchor = Some(0);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft, "draft message");
        assert_eq!(app.copy_requested.as_deref(), Some("draft message"));
        assert!(!app.quit);
    }

    #[test]
    fn ctrl_enter_dispatches_a_steering_message() {
        let mut app = App {
            authenticated: true,
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        app.update_catalog(catalog);
        app.move_selection(true);
        app.sync_history_selection();
        app.insert_draft("Please continue");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        );
        assert!(app.send_requested);
        let request = app.prepare_send().unwrap();
        assert!(request.steer);
        assert_eq!(
            request.payload()["dispatchMode"]["type"],
            "start_immediately"
        );
        assert_eq!(request.payload()["deliveryIntent"], "steer");
    }

    #[test]
    fn mouse_drag_selects_across_unrelated_panels() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::default();
        let event = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), 2, 3),
        );
        handle_mouse(
            &mut app,
            event(MouseEventKind::Drag(MouseButton::Left), 5, 6),
        );
        handle_mouse(&mut app, event(MouseEventKind::Up(MouseButton::Left), 5, 6));
        assert_eq!(app.screen_selection, Some(((3, 2), (6, 6))));
        assert!(!app.screen_selecting);
    }

    #[test]
    fn right_click_menu_offers_paste_without_clearing_selection() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::default();
        app.screen_selection = Some(((1, 2), (1, 5)));
        let event = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Right), 10, 5),
        );
        assert_eq!(app.context_menu, Some((10, 5)));
        assert!(app.screen_selection.is_some());
        app.context_menu_area = ratatui::layout::Rect::new(10, 5, 27, 4);
        handle_mouse(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), 12, 7),
        );
        assert!(app.context_paste_requested);
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn copy_selection_queues_text_for_the_persistent_clipboard_writer() {
        let mut app = App {
            focus: Focus::Chat,
            ..App::default()
        };
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "copy me", ratatui::style::Style::default());
        app.screen_buffer = Some(buffer);
        app.screen_selection = Some(((0, 0), (0, 7)));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.copy_requested.as_deref(), Some("copy me"));
        assert!(!app.quit);
    }

    #[test]
    fn pairing_key_types_normally_in_composer_and_does_not_duplicate_requests() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        let key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        handle_key(&mut app, key);
        assert_eq!(app.draft, "p");
        assert!(!app.pairing_requested);
        app.editing = false;
        app.pairing_in_progress = true;
        handle_key(&mut app, key);
        assert!(!app.pairing_requested);
        app.pairing_in_progress = false;
        handle_key(&mut app, key);
        assert!(app.pairing_requested);
    }

    #[test]
    fn prompt_cursor_edits_unicode_and_navigates_lines() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            ..App::default()
        };
        app.insert_draft("ab\n世界");
        app.move_draft_cursor(KeyCode::Home);
        app.insert_draft("!");
        assert_eq!(app.draft, "ab\n!世界");
        app.move_draft_cursor(KeyCode::Up);
        assert_eq!(app.draft_cursor, 1);
        app.move_draft_cursor(KeyCode::End);
        app.delete_draft();
        assert_eq!(app.draft, "ab!世界");
        app.backspace_draft();
        assert_eq!(app.draft, "a!世界");
    }

    #[test]
    fn shift_d_toggles_dictation_without_conflicting_with_diffs_and_esc_cancels() {
        let mut app = App {
            focus: Focus::Composer,
            ..App::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        );
        assert!(app.dictation.toggle_requested);
        app.dictation.toggle_requested = false;
        app.dictation.phase = crate::dictation::Phase::Recording;
        app.focus = Focus::Chat;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT),
        );
        assert!(app.dictation.toggle_requested);
        app.dictation.toggle_requested = false;
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.dictation.cancel_requested);

        app.dictation = Default::default();
        app.focus = Focus::Composer;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert!(!app.dictation.toggle_requested);
        assert!(app.focus == Focus::Workspace);
        assert_eq!(app.workspace_tab, 1);

        app.focus = Focus::Composer;
        app.editing = true;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        );
        assert_eq!(app.draft, "D");
        assert!(!app.dictation.toggle_requested);
    }

    #[test]
    fn quit_prompts_to_shut_down_local_server_with_no_as_default() {
        let mut app = App {
            local_server_pid: Some(4242),
            ..Default::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(!app.quit);
        assert_eq!(
            app.quit_dialog,
            Some(crate::app::QuitDialog {
                shutdown_server: false
            })
        );

        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.quit_dialog.is_none());
        assert!(!app.quit);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.quit);
        assert!(!app.shutdown_server_on_quit);

        let mut app = App {
            local_server_pid: Some(4242),
            ..Default::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.quit_dialog,
            Some(crate::app::QuitDialog {
                shutdown_server: true
            })
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.quit);
        assert!(app.shutdown_server_on_quit);

        let mut app = App {
            local_server_pid: Some(4242),
            ..Default::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        assert!(app.quit);
        assert!(!app.shutdown_server_on_quit);
    }

    fn always_other_instance() -> bool {
        true
    }

    #[test]
    fn quit_skips_prompt_when_other_tui3_instances_are_running() {
        let mut app = App {
            local_server_pid: Some(4242),
            check_other_instances: Some(always_other_instance),
            ..Default::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(app.quit);
        assert!(app.quit_dialog.is_none());
    }

    #[test]
    fn ctrl_backspace_deletes_word_and_ctrl_a_selects_all_when_editing() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            draft: "hello world rust".into(),
            draft_cursor: 16,
            ..Default::default()
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft, "hello world ");
        assert_eq!(app.draft_cursor, 12);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.draft_selection_anchor, Some(0));
        assert_eq!(app.draft_cursor, 12);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(app.draft, "");
        assert_eq!(app.draft_cursor, 0);
        assert_eq!(app.draft_selection_anchor, None);
    }

    #[test]
    fn enter_while_dictating_stops_and_sends_on_finish() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            draft_thread: Some("t1".into()),
            ..Default::default()
        };
        app.dictation.phase = crate::dictation::Phase::Recording;
        app.dictation.thread = Some("t1".into());
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.editing);
        assert!(app.dictation.toggle_requested);
        assert!(app.dictation.send_on_finish);
        assert!(!app.send_requested);

        app.apply_dictation(crate::dictation::Event::Finished(Ok(
            "ship this message".into()
        )));
        assert_eq!(app.draft, "ship this message");
        assert!(app.send_requested);
        assert!(!app.editing);
    }

    #[test]
    fn enter_on_thread_in_projects_opens_thread_menu_for_rename_and_delete() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.focus = Focus::Projects;

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.thread_menu.is_some());
        assert_eq!(app.thread_menu.as_ref().unwrap().thread_id, "t1");

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.thread_menu.as_ref().unwrap().mode,
            crate::app::ThreadMenuMode::Rename
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.thread_action_requested,
            Some(crate::t3::ThreadAction::Rename {
                thread_id: "t1".into(),
                title: "Conversation!".into(),
            })
        );

        app.thread_action_requested = None;
        app.thread_menu.as_mut().unwrap().busy = false;
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert_eq!(
            app.thread_menu.as_ref().unwrap().mode,
            crate::app::ThreadMenuMode::ConfirmDelete
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        );
        assert_eq!(
            app.thread_action_requested,
            Some(crate::t3::ThreadAction::Delete {
                thread_id: "t1".into(),
            })
        );
    }

    #[test]
    fn slash_command_suggestions_and_clear_sets_fresh_context_on_next_send() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.sync_history_selection();
        app.focus = Focus::Composer;
        app.editing = true;

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );
        assert_eq!(app.matching_slash_commands().len(), 3);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        let matches = app.matching_slash_commands();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].name, "clear");
        assert_eq!(matches[1].name, "context");

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.draft, "");
        assert!(app.is_fresh_context());
        assert!(!app.send_requested);

        app.authenticated = true;
        app.draft = "Start fresh here".into();
        app.draft_cursor = app.draft.chars().count();
        app.steer_requested = true;
        let request = app.prepare_send().unwrap();
        assert!(request.fresh_context);
        assert!(!request.steer);
        assert_eq!(request.payload()["freshContext"], true);
        assert_eq!(
            request.payload()["dispatchMode"]["type"],
            "queue_after_active"
        );

        app.finish_send(request, Ok(()));
        assert!(!app.is_fresh_context());
    }

    #[test]
    fn slash_context_command_enables_context_report_for_selected_thread() {
        let mut app = App::default();
        app.update_catalog(crate::catalog::tests::snapshot());
        app.sidebar_state.select(Some(1));
        app.sync_history_selection();
        app.focus = Focus::Composer;
        app.editing = true;

        for ch in "/context".chars() {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        let matches = app.matching_slash_commands();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "context");

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.draft, "");
        assert_eq!(app.context_report_thread.as_deref(), Some("t1"));
        assert!(!app.send_requested);
    }

    #[test]
    fn settings_row_3_navigates_and_triggers_server_update_action() {
        let mut app = App::default();
        app.authenticated = true;
        app.server_version = Some("0.1.10".into());
        app.server_version_status = crate::app::ServerVersionStatus::UpdateAvailable {
            latest: "0.1.11".into(),
            database_migration: None,
        };
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        assert!(app.settings_open);
        assert_eq!(app.settings_row, 0);
        // Opening settings triggers a fresh version check; restore UpdateAvailable for action test.
        app.server_version_status = crate::app::ServerVersionStatus::UpdateAvailable {
            latest: "0.1.11".into(),
            database_migration: None,
        };
        for _ in 0..5 {
            handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(app.settings_row, 5);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.server_update_requested);
        assert!(matches!(
            app.server_update_status,
            crate::app::ServerUpdateStatus::Downloading { .. }
        ));
    }

    #[test]
    fn settings_telemetry_toggle_updates_and_applies() {
        let mut app = App::default();
        assert!(!app.telemetry_enabled);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        for _ in 0..4 {
            handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(app.settings_row, 4);
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.telemetry_highlight);
        assert!(!app.telemetry_enabled);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.telemetry_enabled);

        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(!app.telemetry_highlight);
        handle_key(&mut app, KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.telemetry_enabled);
    }

    #[test]
    fn alt_enter_dispatches_steering_and_tab_does_not_switch_panes() {
        let mut app = App {
            focus: Focus::Composer,
            editing: true,
            draft: "Steer now".into(),
            ..Default::default()
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(app.focus == Focus::Composer);
        assert!(app.editing);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        assert!(app.steer_requested);
        assert!(app.send_requested);
        assert!(!app.editing);

        app.focus = Focus::Projects;
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(app.focus == Focus::Projects);
    }

    #[test]
    fn settings_layout_preset_updates_workspace_width() {
        let mut app = App::default();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        for _ in 0..3 {
            handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(app.settings_row, 3);
        assert_eq!(
            app.layout_preset,
            crate::preferences::LayoutPreset::Balanced
        );
        assert_eq!(app.workspace_width, 40);
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.layout_highlight,
            crate::preferences::LayoutPreset::Default
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            app.layout_highlight,
            crate::preferences::LayoutPreset::Balanced
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.layout_highlight,
            crate::preferences::LayoutPreset::EditorFocused
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.layout_preset,
            crate::preferences::LayoutPreset::EditorFocused
        );
        assert_eq!(app.workspace_width, 50);
        assert!(app.workspace_visible);

        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.layout_highlight,
            crate::preferences::LayoutPreset::ChatOnly
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.layout_preset,
            crate::preferences::LayoutPreset::ChatOnly
        );
        assert!(!app.workspace_visible);
    }

    #[test]
    fn sidebar_terminal_typing_mode_requires_enter_and_exits_on_esc() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 2,
            workspace_visible: true,
            ..Default::default()
        };
        assert!(!app.local_terminal.tab_typing);

        // Navigation mode: Up moves to WorkspaceTabs, Space/Enter enters typing mode.
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert!(app.local_terminal.tab_typing);

        // In typing mode, conflicting hotkeys are ignored and not handled by TUI.
        for ch in ['q', 's', 'w', 'd', 'e', 'g', 'c', 'm', 'o', 'p'] {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
            assert!(app.focus == Focus::Workspace);
            assert_eq!(app.workspace_tab, 2);
            assert!(app.workspace_visible);
            assert!(!app.settings_open);
            assert!(!app.startup.open);
            assert!(app.quit_dialog.is_none());
        }
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
        );
        assert!(app.quit_dialog.is_none());
        assert!(app.local_terminal.tab_typing);

        // Esc exits typing mode without leaving the Terminal panel.
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.local_terminal.tab_typing);
        assert!(app.focus == Focus::Workspace);

        // Once out of typing mode, navigation works again.
        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(app.focus == Focus::WorkspaceTabs);
    }
}

#[cfg(test)]
mod terminal_overlay_tests {
    use super::*;
    #[test]
    fn overlay_routes_real_typing_to_fish_and_survives_hiding() {
        if !std::path::Path::new("/usr/bin/fish").exists() {
            return;
        }
        let mut app = App {
            terminal_overlay_open: true,
            ..App::default()
        };
        app.local_terminal.session = Some(
            crate::local_terminal::Session::start_shell("/tmp".into(), "/usr/bin/fish".into())
                .unwrap(),
        );
        for c in "printf 'routed_%s_ok\\n' keyx".chars() {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let contents = app
                .local_terminal
                .session
                .as_ref()
                .unwrap()
                .parser
                .lock()
                .unwrap()
                .screen()
                .contents();
            if contents.contains("routed_keys_ok") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "typed command did not execute: {contents}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for _ in 0..2 {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            );
        }
        assert!(app.terminal_overlay_open);
        assert!(app.local_terminal.session.is_some());
        assert!(!app.quit);
    }
    #[test]
    fn overlay_keeps_escape_interrupt_and_text_inside_terminal() {
        let mut app = App::default();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        );
        assert!(app.terminal_overlay_open);
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT),
        ] {
            handle_key(&mut app, key);
            assert!(app.terminal_overlay_open);
            assert!(!app.quit);
            assert!(app.quit_dialog.is_none());
        }
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        );
        assert!(!app.terminal_overlay_open);
    }

    #[test]
    fn file_editor_isolates_from_global_hotkeys_while_explorer_allows_them() {
        let mut app = App {
            focus: Focus::Workspace,
            workspace_tab: 0,
            ..Default::default()
        };
        app.editor.open_file = Some("main.rs".into());
        app.editor.lines = vec!["fn main() {}".into()];
        app.editor.tree_focus = false;

        // In Regular file editor, 'q', 's', and Ctrl+J must not trigger global hotkeys.
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        );
        assert!(app.quit_dialog.is_none());
        assert!(!app.settings_open);
        assert!(!app.terminal_overlay_open);
        assert_eq!(app.editor.lines, vec!["qsfn main() {}"]);

        // Returning to explorer (tree_focus = true) allows global hotkeys again.
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.editor.tree_focus);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        assert!(app.settings_open);
    }
}
