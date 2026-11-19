//! One layout for the draft's text, cursor, and visible rows.
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn layout(text: &str, cursor: usize, width: usize) -> (Vec<String>, (usize, usize)) {
    if width == 0 {
        return (vec![], (0, 0));
    }
    let mut lines = vec![String::new()];
    let (mut row, mut column, mut position) = (0, 0, 0);
    let mut caret = None;
    for grapheme in text.graphemes(true) {
        if grapheme != "\n" && column + grapheme.width() > width {
            row += 1;
            column = 0;
            lines.push(String::new());
        }
        if position >= cursor && caret.is_none() {
            caret = Some((row, column));
        }
        if grapheme == "\n" {
            row += 1;
            column = 0;
            lines.push(String::new());
        } else {
            lines[row].push_str(grapheme);
            column += grapheme.width();
        }
        position += grapheme.chars().count();
    }
    if column >= width {
        row += 1;
        column = 0;
        lines.push(String::new());
    }
    (lines, caret.unwrap_or((row, column)))
}

pub fn highlight_selection(
    frame: &mut Frame,
    text: &str,
    cursor: usize,
    anchor: usize,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (_, (cursor_row, _)) = layout(text, cursor, area.width as usize);
    let scroll = cursor_row.saturating_sub(area.height.saturating_sub(1) as usize);
    let start = anchor.min(cursor);
    let end = anchor.max(cursor);
    let (mut row, mut col, mut offset) = (0, 0, 0);
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if grapheme != "\n" && col + width > area.width as usize {
            row += 1;
            col = 0;
        }
        if offset < end
            && offset + grapheme.chars().count() > start
            && row >= scroll
            && row < scroll + area.height as usize
        {
            for dx in 0..width.max(1).min(area.width as usize - col) {
                let x = area.x + (col + dx) as u16;
                let y = area.y + (row - scroll) as u16;
                if x < area.right() {
                    frame.buffer_mut()[(x, y)].set_bg(ratatui::style::Color::Blue);
                }
            }
        }
        if grapheme == "\n" {
            row += 1;
            col = 0;
        } else {
            col += width;
        }
        offset += grapheme.chars().count();
    }
}

pub fn draw(
    frame: &mut Frame,
    text: &str,
    cursor: usize,
    editing: bool,
    block: Block<'_>,
    area: Rect,
) {
    let inner = block.inner(area);
    let (lines, (row, column)) = layout(text, cursor, inner.width as usize);
    let scroll = if editing {
        row.saturating_sub(inner.height.saturating_sub(1) as usize)
    } else {
        lines.len().saturating_sub(inner.height as usize)
    };
    let visible = lines
        .into_iter()
        .skip(scroll)
        .take(inner.height as usize)
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(Paragraph::new(visible).block(block), area);
    if editing && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((
            inner.x + column.min(inner.width as usize - 1) as u16,
            inner.y + row.saturating_sub(scroll).min(inner.height as usize - 1) as u16,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_uses_the_same_wrap_as_text_and_has_space_at_the_end() {
        let (lines, cursor) = layout("ab cd ef", 7, 5);
        assert_eq!(lines, ["ab cd", " ef"]);
        assert_eq!(cursor, (1, 2));
        assert_eq!(layout("abcde", 5, 5).1, (1, 0));
        assert_eq!(layout("世界a", 2, 3).1, (1, 2));
    }

    #[test]
    fn scrolling_follows_cursor_and_backspacing_clears_every_visible_character() {
        use ratatui::{Terminal, backend::TestBackend, widgets::Block};
        let mut app = crate::app::App::default();
        app.insert_draft("ab cd ef gh ij kl mn");
        let mut terminal = Terminal::new(TestBackend::new(8, 4)).unwrap();
        app.move_draft_cursor(crossterm::event::KeyCode::Home);
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app.draft,
                    app.draft_cursor,
                    true,
                    Block::bordered(),
                    frame.area(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 1)].symbol(), "a");
        app.move_draft_cursor(crossterm::event::KeyCode::End);
        while !app.draft.is_empty() {
            app.backspace_draft();
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        &app.draft,
                        app.draft_cursor,
                        true,
                        Block::bordered(),
                        frame.area(),
                    )
                })
                .unwrap();
        }
        for y in 1..3 {
            for x in 1..7 {
                assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), " ");
            }
        }
    }
}
