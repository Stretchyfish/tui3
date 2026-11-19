//! Conversation presentation is independent of T3's transcript and event handling.
use crate::history::Transcript;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

/// Cache wrapped conversation cells so scrolling never reflows the entire history.
pub struct Viewport {
    pub content: Text<'static>,
    pub buffer: ratatui::buffer::Buffer,
    heights: Vec<usize>,
}

fn prepared_line(line: &Line<'static>, width: u16) -> (Line<'static>, u16) {
    let mut body = line.clone();
    if let Some(first) = body.spans.first_mut()
        && let Some(rest) = first
            .content
            .strip_prefix("│ ")
            .or_else(|| first.content.strip_prefix('│'))
    {
        first.content = rest.to_owned().into();
        return (body, width.min(2));
    }
    (body, 0)
}

fn wrapped_height(line: &Line<'static>, width: u16) -> usize {
    use ratatui::widgets::{Paragraph, Wrap};
    if width == 0 {
        return 0;
    }
    let (body, gutter) = prepared_line(line, width);
    if width <= gutter {
        return 1;
    }
    Paragraph::new(Text::from(body))
        .wrap(Wrap { trim: false })
        .line_count(width - gutter)
}

fn render_lines(
    buffer: &mut ratatui::buffer::Buffer,
    content: &Text<'static>,
    heights: &[usize],
    start: usize,
    mut row: usize,
) {
    use ratatui::{
        layout::Rect,
        widgets::{Paragraph, Widget, Wrap},
    };
    let width = buffer.area.width;
    for (line, height) in content.lines[start..].iter().zip(&heights[start..]) {
        if row >= buffer.area.height as usize {
            break;
        }
        let height = (*height).min(buffer.area.height as usize - row) as u16;
        let (body, gutter) = prepared_line(line, width);
        let area = Rect::new(gutter, row as u16, width.saturating_sub(gutter), height);
        Paragraph::new(Text::from(body))
            .style(content.style)
            .wrap(Wrap { trim: false })
            .render(area, buffer);
        if gutter > 0 {
            let style = content.style.patch(line.style).patch(line.spans[0].style);
            for y in row as u16..row as u16 + height {
                buffer[(0, y)].set_symbol("│").set_style(style);
            }
        }
        row += height as usize;
    }
}

impl Viewport {
    pub fn same_source_line(&self, first_row: u16, second_row: u16) -> bool {
        let mut start = 0usize;
        for height in &self.heights {
            let end = start + height;
            if (first_row as usize) < end {
                return (second_row as usize) < end;
            }
            start = end;
        }
        false
    }
    pub fn new(content: Text<'static>, width: u16) -> Self {
        let heights: Vec<_> = content
            .lines
            .iter()
            .map(|line| wrapped_height(line, width))
            .collect();
        let height = heights.iter().sum::<usize>().min(u16::MAX as usize) as u16;
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        render_lines(&mut buffer, &content, &heights, 0, 0);
        Self {
            content,
            buffer,
            heights,
        }
    }

    pub fn update(&mut self, content: Text<'static>, width: u16) {
        if self.buffer.area.width != width {
            *self = Self::new(content, width);
            return;
        }
        if self.content == content {
            return;
        }
        let prefix = if self.content.style == content.style {
            self.content
                .lines
                .iter()
                .zip(&content.lines)
                .take_while(|(a, b)| a == b)
                .count()
        } else {
            0
        };
        let row = self.heights[..prefix]
            .iter()
            .sum::<usize>()
            .min(u16::MAX as usize);
        self.heights.truncate(prefix);
        self.heights.extend(
            content.lines[prefix..]
                .iter()
                .map(|line| wrapped_height(line, width)),
        );
        let height = self.heights.iter().sum::<usize>().min(u16::MAX as usize) as u16;
        self.buffer
            .resize(ratatui::layout::Rect::new(0, 0, width, height));
        let offset = (row * width as usize).min(self.buffer.content.len());
        self.buffer.content[offset..].fill(ratatui::buffer::Cell::EMPTY);
        render_lines(&mut self.buffer, &content, &self.heights, prefix, row);
        self.content = content;
    }
}

pub fn conversation(
    history: &Transcript,
    runs: &std::collections::HashMap<String, crate::history::Run>,
    working: Option<std::time::Duration>,
    diffs: &std::collections::HashMap<String, crate::history::FileCounts>,
) -> Text<'static> {
    let messages = history.messages();
    if messages.is_empty() && working.is_none() {
        return Text::from("No chat messages in this thread yet.");
    }
    let mut lines = Vec::new();
    let last_by_run: std::collections::HashMap<_, _> = messages
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.item.kind == "assistant_message")
        .map(|(index, entry)| (entry.item.run_id.as_deref(), index))
        .collect();
    let working_message = messages.iter().rposition(|entry| {
        entry.item.kind == "assistant_message"
            && (entry.item.streaming
                || entry
                    .item
                    .run_id
                    .as_ref()
                    .and_then(|id| runs.get(id))
                    .is_some_and(|run| run.active()))
    });
    for (index, entry) in messages.iter().enumerate() {
        let item = &entry.item;
        let queued = item.kind == "user_message"
            && (matches!(item.status.as_deref(), Some("pending" | "queued"))
                || item
                    .run_id
                    .as_ref()
                    .and_then(|id| runs.get(id))
                    .is_some_and(|run| run.status == "queued"));
        let steered = matches!(
            item.input_intent.as_deref(),
            Some("steer" | "promoted_queued_to_steer")
        );
        let (label, accent, body) = match item.kind.as_str() {
            "user_message" => (
                if queued && steered {
                    "YOU · steer · queued"
                } else if queued {
                    "YOU · queued"
                } else if steered {
                    "YOU · steer"
                } else {
                    "YOU"
                },
                Color::Cyan,
                Color::Rgb(190, 225, 235),
            ),
            "assistant_message" => ("AGENT", Color::Green, Color::Rgb(225, 228, 235)),
            _ => ("SYSTEM", Color::Yellow, Color::Gray),
        };
        let mut header = vec![Span::styled(
            format!("╭─ {label} "),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )];
        if working_message == Some(index)
            && let Some(duration) = working
        {
            lines.push(Line::styled(
                format!("Working for {}s", duration.as_secs()),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if item.kind == "assistant_message"
            && last_by_run.get(&item.run_id.as_deref()) == Some(&index)
            && let Some(run) = item.run_id.as_ref().and_then(|id| runs.get(id))
            && !run.active()
            && let Some(duration) = run.duration()
        {
            lines.push(Line::styled(
                format!("Worked for {}s", duration.as_secs()),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if item.streaming {
            header.push(Span::styled(
                "· responding…",
                Style::default().fg(Color::Yellow),
            ));
        }
        lines.push(Line::from(header));
        // Preserve whitespace and code indentation; wrapping remains the viewport's job.
        for line in item.text.as_deref().unwrap_or_default().split('\n') {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(accent)),
                Span::styled(line.to_owned(), Style::default().fg(body)),
            ]));
        }
        if !item.attachments.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(accent)),
                Span::styled(
                    format!(
                        "[{} attachment(s); preview unavailable]",
                        item.attachments.len()
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        if item.kind == "assistant_message"
            && let Some(run_id) = item.run_id.as_deref()
            && last_by_run.get(&Some(run_id)) == Some(&index)
        {
            changed_files(&mut lines, history, run_id, diffs.get(run_id));
        }
        lines.push(Line::styled("╰─", Style::default().fg(accent)));
        lines.push(Line::default());
    }
    if working_message.is_none()
        && let Some(duration) = working
    {
        lines.push(Line::styled(
            format!("Working for {}s", duration.as_secs()),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::styled(
            "╭─ AGENT",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Text::from(lines)
}

fn changed_files(
    lines: &mut Vec<Line<'static>>,
    history: &Transcript,
    run_id: &str,
    diff: Option<&crate::history::FileCounts>,
) {
    let mut files = std::collections::BTreeMap::<&str, (Option<u64>, Option<u64>)>::new();
    if diff.is_none() {
        for entry in &history.items {
            let item = &entry.item;
            if item.kind != "file_change"
                || item.run_id.as_deref() != Some(run_id)
                || matches!(
                    item.status.as_deref(),
                    Some("failed" | "cancelled" | "interrupted")
                )
            {
                continue;
            }
            if let Some(path) = item.file_name.as_deref() {
                let counts = files.entry(path).or_default();
                *counts = (None, None);
            }
        }
    }
    if let Some(diff) = diff {
        files = diff
            .iter()
            .map(|(path, (added, removed))| (path.as_str(), (Some(*added), Some(*removed))))
            .collect();
    }
    if files.is_empty() {
        return;
    }
    lines.push(Line::styled("│", Style::default().fg(Color::Green)));
    let complete = files
        .values()
        .all(|(added, removed)| added.is_some() && removed.is_some());
    let mut heading = vec![Span::styled(
        format!(
            "│ {} file{}",
            if complete {
                format!("{} changed", files.len())
            } else {
                format!("Changing {}", files.len())
            },
            if files.len() == 1 { "" } else { "s" }
        ),
        Style::default().fg(Color::Green),
    )];
    if complete {
        let added = files.values().fold(0u64, |total, counts| {
            total.saturating_add(counts.0.unwrap_or_default())
        });
        let removed = files.values().fold(0u64, |total, counts| {
            total.saturating_add(counts.1.unwrap_or_default())
        });
        heading.push(Span::styled(
            format!("  +{added}"),
            Style::default().fg(Color::Green),
        ));
        heading.push(Span::styled(
            format!("  -{removed}"),
            Style::default().fg(Color::Red),
        ));
    }
    lines.push(Line::from(heading));
    for (path, (additions, deletions)) in files {
        let mut spans = vec![
            Span::styled("│ ", Style::default().fg(Color::Green)),
            Span::styled(
                path.to_owned(),
                Style::default().fg(Color::Rgb(225, 228, 235)),
            ),
        ];
        if let Some(n) = additions {
            spans.push(Span::styled(
                format!("  +{n}"),
                Style::default().fg(Color::Green),
            ));
        } else {
            spans.push(Span::styled("  +?", Style::default().fg(Color::Green)));
        }
        if let Some(n) = deletions {
            spans.push(Span::styled(
                format!("  -{n}"),
                Style::default().fg(Color::Red),
            ));
        } else {
            spans.push(Span::styled("  -?", Style::default().fg(Color::Red)));
        }
        lines.push(Line::from(spans));
    }
}

pub fn notice(content: &mut Text<'static>, message: impl Into<String>, color: Color) {
    content.lines.push(Line::default());
    // Notices may contain newlines, so keep each on its own display line.
    let message = message.into();
    content.lines.extend(
        message
            .split('\n')
            .map(|line| Line::styled(line.to_owned(), Style::default().fg(color))),
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn steering_user_messages_are_labeled_from_t3_input_intent() {
        let history = serde_json::from_value(serde_json::json!({
            "threadId": "t1", "title": "Chat", "items": [
                {"position": 0, "item": {"type": "user_message", "text": "Continue", "inputIntent": "steer"}},
                {"position": 1, "item": {"type": "user_message", "text": "Next question", "inputIntent": "turn_start"}}
            ]
        })).unwrap();
        let text = super::conversation(&history, &Default::default(), None, &Default::default())
            .to_string();
        assert!(text.contains("YOU · steer"));
        assert_eq!(text.matches("YOU · steer").count(), 1);
        assert_eq!(text.matches("╭─ YOU").count(), 2);
    }

    #[test]
    fn queued_user_message_has_a_waiting_label() {
        let history = serde_json::from_value(serde_json::json!({
            "threadId": "t1", "title": "Chat", "items": [
                {"position": 0, "item": {"type": "user_message", "text": "Next", "runId": "r2"}}
            ]
        }))
        .unwrap();
        let run: crate::history::Run =
            serde_json::from_value(serde_json::json!({"id": "r2", "status": "queued"})).unwrap();
        let runs = [(run.id.clone(), run)].into_iter().collect();
        let text = super::conversation(&history, &runs, None, &Default::default()).to_string();
        assert!(text.contains("YOU · queued"));
    }

    #[test]
    fn message_gutter_continues_on_wrapped_rows_during_streaming_and_resize() {
        use ratatui::{
            style::{Color, Style},
            text::{Line, Span, Text},
        };
        for color in [Color::Green, Color::Cyan] {
            let text = |body: &str| {
                Text::from(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(color)),
                    Span::raw(body.to_owned()),
                ]))
            };
            let mut viewport =
                super::Viewport::new(text("A message that wraps across several rows."), 12);
            for width in [12, 8, 20] {
                viewport.update(
                    text("A message that wraps across several rows. More streamed text."),
                    width,
                );
                assert!(viewport.buffer.area.height > 1);
                for row in 0..viewport.buffer.area.height {
                    assert_eq!(viewport.buffer[(0, row)].symbol(), "│");
                    assert_eq!(viewport.buffer[(0, row)].fg, color);
                    assert_eq!(viewport.buffer[(1, row)].symbol(), " ");
                }
            }
        }
    }
    #[test]
    fn incremental_wrapping_matches_full_render_after_edits_and_resize() {
        use ratatui::text::Text;
        let mut cache = super::Viewport::new(
            Text::from("Header\nlong line with wrapping and 世界\nold tail"),
            12,
        );
        for (text, width) in [
            (
                "Header\nlong line with wrapping and 世界\nnew tail with more text",
                12,
            ),
            ("Header\nshort", 12),
            ("Header\nshort", 5),
            ("", 5),
        ] {
            let content = Text::from(text);
            cache.update(content.clone(), width);
            let fresh = super::Viewport::new(content, width);
            assert_eq!(cache.buffer, fresh.buffer);
        }
    }
    #[test]
    fn changed_files_are_grouped_by_run_and_paths_are_unique() {
        let history = serde_json::from_value(serde_json::json!({
            "threadId":"t1", "title":"Chat", "items":[
                {"position":0,"item":{"id":"a1","type":"assistant_message","runId":"r1","text":"Done"}},
                {"position":1,"item":{"id":"f1","type":"file_change","runId":"r1","fileName":"src/main.rs","additions":3,"deletions":1}},
                {"position":2,"item":{"id":"f2","type":"file_change","runId":"r1","fileName":"src/main.rs","additions":2,"deletions":0}},
                {"position":3,"item":{"id":"f3","type":"file_change","runId":"r2","fileName":"other.rs","additions":99,"deletions":99}},
                {"position":4,"item":{"id":"a2","type":"assistant_message","runId":"r1","text":"Summary"}}
            ]
        })).unwrap();
        let counts = [(
            "r1".into(),
            [
                ("src/main.rs".into(), (2, 1)),
                ("generated.rs".into(), (7, 0)),
            ]
            .into(),
        )]
        .into();
        let text = super::conversation(&history, &Default::default(), None, &counts).to_string();
        assert!(text.contains("2 changed files  +9  -1"));
        assert!(text.contains("src/main.rs  +2  -1"));
        assert!(text.contains("generated.rs  +7  -0"));
        assert_eq!(text.matches("src/main.rs").count(), 1);
        assert!(!text.contains("other.rs"));
        assert!(text.find("Summary").unwrap() < text.find("changed file").unwrap());
    }
    #[test]
    fn completed_run_duration_appears_in_agent_header() {
        let mut history = crate::history::tests::transcript("t1");
        let message = &mut history.items[0].item;
        message.run_id = Some("r1".into());
        message.streaming = false;
        let run = serde_json::from_value(serde_json::json!({
            "id":"r1", "status":"completed",
            "startedAt":"2026-11-18T10:00:00Z",
            "completedAt":"2026-11-18T10:00:42Z"
        }))
        .unwrap();
        let text = super::conversation(
            &history,
            &[("r1".into(), run)].into(),
            None,
            &Default::default(),
        );
        assert!(
            text.lines
                .iter()
                .any(|line| line.to_string().contains("Worked for 42s"))
        );
    }
}
