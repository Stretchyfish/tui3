//! Conversation presentation is independent of T3's transcript and event handling.
use crate::history::Transcript;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

pub fn conversation(
    history: &Transcript,
    runs: &std::collections::HashMap<String, crate::history::Run>,
) -> Text<'static> {
    let messages = history.messages();
    if messages.is_empty() {
        return Text::from("No chat messages in this thread yet.");
    }
    let mut lines = Vec::new();
    for (index, entry) in messages.iter().enumerate() {
        let item = &entry.item;
        let (label, accent, body) = match item.kind.as_str() {
            "user_message" => ("YOU", Color::Cyan, Color::Rgb(190, 225, 235)),
            "assistant_message" => ("AGENT", Color::Green, Color::Rgb(225, 228, 235)),
            _ => ("SYSTEM", Color::Yellow, Color::Gray),
        };
        let mut header = vec![Span::styled(
            format!("╭─ {label} "),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )];
        if item.kind == "assistant_message"
            && !messages[index + 1..].iter().any(|later| {
                later.item.kind == "assistant_message" && later.item.run_id == item.run_id
            })
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
        lines.push(Line::styled("╰─", Style::default().fg(accent)));
        lines.push(Line::default());
    }
    Text::from(lines)
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
        let text = super::conversation(&history, &[("r1".into(), run)].into());
        assert!(
            text.lines
                .iter()
                .any(|line| line.to_string().contains("Worked for 42s"))
        );
    }
}
