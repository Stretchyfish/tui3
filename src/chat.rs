//! Conversation presentation is independent of T3's transcript and event handling.
use crate::history::Transcript;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

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
        let (label, accent, body) = match item.kind.as_str() {
            "user_message" => ("YOU", Color::Cyan, Color::Rgb(190, 225, 235)),
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
        if item.kind == "assistant_message"
            && let Some(run_id) = item.run_id.as_deref()
            && !messages[index + 1..].iter().any(|later| {
                later.item.kind == "assistant_message"
                    && later.item.run_id.as_deref() == Some(run_id)
            })
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
    let mut heading = vec![Span::styled(
        format!(
            "│ {} changed file{}",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ),
        Style::default().fg(Color::Green),
    )];
    if files
        .values()
        .all(|(added, removed)| added.is_some() && removed.is_some())
    {
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
