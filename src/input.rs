use crate::app::{App, Focus};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    use crate::app::ClickTarget;
    use crossterm::event::{MouseButton, MouseEventKind};
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
        return;
    }
    match target {
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
            app.style_highlight = style;
            app.typing_style = style;
        }
        None => (),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(key.code, KeyCode::Char('c' | 'q')) {
            app.quit = true;
        }
        return;
    }
    if app.settings_open {
        match key.code {
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
            KeyCode::Enter => app.models.change_requested = true,
            KeyCode::Down | KeyCode::Char('j') => app.move_model(true, 1),
            KeyCode::Up | KeyCode::Char('k') => app.move_model(false, 1),
            KeyCode::PageDown => app.move_model(true, 10),
            KeyCode::PageUp => app.move_model(false, 10),
            KeyCode::Home => app.move_model(false, usize::MAX),
            KeyCode::End => app.move_model(true, usize::MAX),
            KeyCode::Char('r') if !app.models.loading => app.request_models(),
            _ => (),
        }
        return;
    }
    if app.editing {
        match key.code {
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                app.insert_draft("\n")
            }
            KeyCode::Enter => app.send_requested = true,
            KeyCode::Esc => app.editing = false,
            KeyCode::Tab | KeyCode::BackTab => {
                app.editing = false;
                if key.code == KeyCode::BackTab {
                    app.previous_focus();
                } else {
                    app.toggle_focus();
                }
            }
            KeyCode::Char(character) => app.insert_draft(&character.to_string()),
            KeyCode::Backspace => app.backspace_draft(),
            KeyCode::Delete => app.delete_draft(),
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End => app.move_draft_cursor(key.code),
            _ => (),
        }
        return;
    }
    match key.code {
        KeyCode::Char('s') => {
            app.style_highlight = app.typing_style;
            app.settings_open = true;
        }
        KeyCode::Char('m') => app.focus = Focus::Model,
        KeyCode::Right | KeyCode::Char('l')
            if app.focus == Focus::Chat || app.focus == Focus::Model =>
        {
            app.focus = Focus::Workspace;
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Workspace => {
            app.focus = Focus::WorkspaceTabs;
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::WorkspaceTabs => {
            app.focus = Focus::Workspace;
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::WorkspaceTabs => {
            app.workspace_highlight = (app.workspace_highlight + 1) % 6;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::WorkspaceTabs => {
            app.workspace_highlight = (app.workspace_highlight + 5) % 6;
        }
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::WorkspaceTabs => {
            app.workspace_tab = app.workspace_highlight;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Workspace => {
            app.focus = Focus::Chat;
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k')
            if matches!(app.focus, Focus::Workspace | Focus::WorkspaceTabs) => {}
        KeyCode::Enter if app.focus == Focus::Model => app.open_models(),
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Composer => {
            app.focus = Focus::Model
        }
        KeyCode::Up | KeyCode::Char('k') if app.focus == Focus::Model => {
            app.focus = Focus::Composer
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Composer => {
            app.focus = Focus::Workspace
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Model => {
            app.focus = Focus::Composer
        }
        KeyCode::Down | KeyCode::Char('j') if app.focus == Focus::Model => (),
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
        KeyCode::Tab => app.toggle_focus(),
        KeyCode::BackTab => app.previous_focus(),
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
        KeyCode::Enter if app.focus == Focus::Projects => app.focus = Focus::Chat,
        KeyCode::Down | KeyCode::Char('j') => app.focus = Focus::Composer,
        KeyCode::Up | KeyCode::Char('k') => app.focus = Focus::Chat,
        KeyCode::Enter | KeyCode::Char('i') if app.focus == Focus::Composer => {
            app.editing = true;
        }
        KeyCode::Char('q') => app.quit = true,
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.focus == Focus::WorkspaceTabs);
        for index in 1..=6 {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
            );
            assert_eq!(app.workspace_highlight, index % 6);
            assert_eq!(app.workspace_tab, 0);
        }
        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.workspace_highlight, 5);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.workspace_tab, 5);
        assert!(!app.editing);
        assert!(app.draft.is_empty());
        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
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
    fn enter_sends_only_while_typing_and_alt_enter_adds_a_newline() {
        let mut app = App {
            focus: Focus::Composer,
            ..App::default()
        };
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.editing);
        assert!(!app.send_requested);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        assert_eq!(app.draft, "\n");
        assert!(!app.send_requested);
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.send_requested);
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
}
