//! Lightweight Markdown and code-block renderer for chat messages.
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

const CODE_BG: Color = Color::Rgb(28, 32, 42);
const INLINE_CODE_BG: Color = Color::Rgb(36, 41, 54);

pub fn render(text: &str, accent: Color, body: Color) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let gutter = || Span::styled("│ ", Style::default().fg(accent));
    let base = Style::default().fg(body);
    let mut in_code: Option<String> = None;

    for raw_line in text.split('\n') {
        let trimmed = raw_line.trim();
        if let Some(fence_rest) = trimmed.strip_prefix("```") {
            if in_code.is_some() {
                in_code = None;
                out.push(Line::from(vec![
                    gutter(),
                    Span::styled("└─", Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                let lang = fence_rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let label = if lang.is_empty() { "code" } else { lang.as_str() };
                out.push(Line::from(vec![
                    gutter(),
                    Span::styled("┌─ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{label} "),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                in_code = Some(lang);
            }
            continue;
        }

        if let Some(lang) = &in_code {
            let mut spans = vec![
                gutter(),
                Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
            ];
            spans.extend(highlight_code_line(raw_line, lang));
            out.push(Line::from(spans));
            continue;
        }

        if is_horizontal_rule(trimmed) {
            out.push(Line::from(vec![
                gutter(),
                Span::styled(
                    "────────────────────────",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            continue;
        }

        if let Some((level, heading)) = parse_heading(trimmed) {
            let heading_style = match level {
                1 => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                2 => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            };
            let mut spans = vec![gutter()];
            spans.extend(parse_inline(heading, heading_style));
            out.push(Line::from(spans));
            continue;
        }

        if let Some(quote) = trimmed.strip_prefix("> ").or_else(|| {
            if trimmed == ">" { Some("") } else { None }
        }) {
            let mut spans = vec![
                gutter(),
                Span::styled("▎ ", Style::default().fg(Color::Cyan)),
            ];
            spans.extend(parse_inline(
                quote,
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            ));
            out.push(Line::from(spans));
            continue;
        }

        if let Some((indent, marker, marker_color, rest)) = parse_list_item(raw_line) {
            let mut spans = vec![gutter()];
            if !indent.is_empty() {
                spans.push(Span::raw(indent.to_owned()));
            }
            spans.push(Span::styled(
                marker,
                Style::default()
                    .fg(marker_color)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.extend(parse_inline(rest, base));
            out.push(Line::from(spans));
            continue;
        }

        let mut spans = vec![gutter()];
        spans.extend(parse_inline(raw_line, base));
        out.push(Line::from(spans));
    }

    if out.is_empty() {
        out.push(Line::from(vec![gutter(), Span::styled("", base)]));
    }
    out
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.chars().next().unwrap_or(' ');
    matches!(first, '-' | '*' | '_')
        && trimmed.chars().all(|c| c == first || c == ' ')
        && trimmed.chars().filter(|&c| c == first).count() >= 3
}

fn parse_heading(trimmed: &str) -> Option<(usize, &str)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed[hashes..].strip_prefix(' ')?;
    Some((hashes, rest.trim()))
}

fn parse_list_item(line: &str) -> Option<(&str, String, Color, &str)> {
    let indent_len = line.bytes().take_while(|&b| b == b' ').count();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    if let Some(after) = rest
        .strip_prefix("- [x] ")
        .or_else(|| rest.strip_prefix("- [X] "))
        .or_else(|| rest.strip_prefix("* [x] "))
        .or_else(|| rest.strip_prefix("* [X] "))
    {
        return Some((indent, "☑ ".into(), Color::Green, after));
    }
    if let Some(after) = rest
        .strip_prefix("- [ ] ")
        .or_else(|| rest.strip_prefix("* [ ] "))
    {
        return Some((indent, "☐ ".into(), Color::Yellow, after));
    }
    if let Some(after) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
        return Some((indent, "• ".into(), Color::Cyan, after));
    }
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 4 {
        if let Some(after) = rest[digits..].strip_prefix(". ") {
            return Some((
                indent,
                format!("{}. ", &rest[..digits]),
                Color::Cyan,
                after,
            ));
        }
    }
    None
}

fn parse_inline(text: &str, base: Style) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::styled(String::new(), base)];
    }
    let mut spans = Vec::new();
    let mut buf = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    let flush = |buf: &mut String, spans: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), base));
        }
    };

    while i < bytes.len() {
        // Inline code: `...`
        if bytes[i] == b'`'
            && let Some(end) = text[i + 1..].find('`')
        {
            flush(&mut buf, &mut spans);
            let code = &text[i + 1..i + 1 + end];
            spans.push(Span::styled(
                format!(" {code} "),
                Style::default().fg(Color::Yellow).bg(INLINE_CODE_BG),
            ));
            i += end + 2;
            continue;
        }

        // Markdown link: [label](url)
        if bytes[i] == b'['
            && let Some((label, url, consumed)) = parse_link(&text[i..])
        {
            flush(&mut buf, &mut spans);
            spans.push(Span::styled(
                label.to_owned(),
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            if (url.starts_with("http://") || url.starts_with("https://")) && label != url {
                spans.push(Span::styled(
                    format!(" ({url})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            i += consumed;
            continue;
        }

        // Bare URL: https://... or http://...
        if (text[i..].starts_with("https://") || text[i..].starts_with("http://"))
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        {
            let rest = &text[i..];
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\''))
                .unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            if !url.is_empty() {
                flush(&mut buf, &mut spans);
                spans.push(Span::styled(
                    url.to_owned(),
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                i += url.len();
                continue;
            }
        }

        // Bold+Italic: ***...***
        if text[i..].starts_with("***")
            && let Some(end) = text[i + 3..].find("***")
            && end > 0
        {
            flush(&mut buf, &mut spans);
            let inner = &text[i + 3..i + 3 + end];
            spans.extend(parse_inline(
                inner,
                base.fg(Color::White)
                    .add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ));
            i += end + 6;
            continue;
        }

        // Bold: **...**
        if text[i..].starts_with("**")
            && let Some(end) = text[i + 2..].find("**")
            && end > 0
        {
            flush(&mut buf, &mut spans);
            let inner = &text[i + 2..i + 2 + end];
            spans.extend(parse_inline(
                inner,
                base.fg(Color::White).add_modifier(Modifier::BOLD),
            ));
            i += end + 4;
            continue;
        }

        // Strikethrough: ~~...~~
        if text[i..].starts_with("~~")
            && let Some(end) = text[i + 2..].find("~~")
            && end > 0
        {
            flush(&mut buf, &mut spans);
            let inner = &text[i + 2..i + 2 + end];
            spans.push(Span::styled(
                inner.to_owned(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT),
            ));
            i += end + 4;
            continue;
        }

        // Italic: *...* (avoid matching standalone `* ` or `2 * 3`)
        if bytes[i] == b'*'
            && i + 1 < bytes.len()
            && !bytes[i + 1].is_ascii_whitespace()
            && let Some(end) = text[i + 1..].find('*')
            && end > 0
            && !text[i + 1..i + 1 + end].ends_with(char::is_whitespace)
        {
            flush(&mut buf, &mut spans);
            let inner = &text[i + 1..i + 1 + end];
            spans.extend(parse_inline(inner, base.add_modifier(Modifier::ITALIC)));
            i += end + 2;
            continue;
        }

        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut buf, &mut spans);
    spans
}

fn parse_link(s: &str) -> Option<(&str, &str, usize)> {
    let rest = s.strip_prefix('[')?;
    let close_bracket = rest.find(']')?;
    let label = &rest[..close_bracket];
    if label.is_empty() || label.contains('\n') {
        return None;
    }
    let after = &rest[close_bracket + 1..];
    let after = after.strip_prefix('(')?;
    let close_paren = after.find(')')?;
    let url = &after[..close_paren];
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    let consumed = 1 + close_bracket + 2 + close_paren + 1;
    Some((label, url, consumed))
}

fn highlight_code_line(line: &str, lang: &str) -> Vec<Span<'static>> {
    let bg = Style::default().bg(CODE_BG).fg(Color::Rgb(220, 224, 234));
    if line.is_empty() {
        return vec![Span::styled(" ".to_owned(), bg)];
    }
    let is_shell = matches!(lang, "bash" | "sh" | "zsh" | "shell" | "fish");
    let is_hash_comment = is_shell || matches!(lang, "py" | "python" | "toml" | "yaml" | "yml");

    let mut spans = Vec::new();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_pos, ch) = chars[idx];

        // Line comments
        if (is_hash_comment && ch == '#')
            || (!is_shell && ch == '/' && chars.get(idx + 1).is_some_and(|(_, c)| *c == '/'))
        {
            spans.push(Span::styled(
                line[byte_pos..].to_owned(),
                bg.fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ));
            break;
        }

        // Strings: "..." or '...'
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start = byte_pos;
            idx += 1;
            while idx < chars.len() {
                let (_, c) = chars[idx];
                if c == '\\' && idx + 1 < chars.len() {
                    idx += 2;
                    continue;
                }
                idx += 1;
                if c == quote {
                    break;
                }
            }
            let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
            spans.push(Span::styled(
                line[start..end].to_owned(),
                bg.fg(Color::Green),
            ));
            continue;
        }

        // Shell variables: $VAR / ${VAR}
        if is_shell && ch == '$' {
            let start = byte_pos;
            idx += 1;
            while idx < chars.len()
                && (chars[idx].1.is_ascii_alphanumeric()
                    || matches!(chars[idx].1, '_' | '{' | '}'))
            {
                idx += 1;
            }
            let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
            spans.push(Span::styled(
                line[start..end].to_owned(),
                bg.fg(Color::Magenta),
            ));
            continue;
        }

        // Shell flags: -x / --foo-bar
        if is_shell
            && ch == '-'
            && (idx == 0 || chars[idx - 1].1.is_whitespace())
            && chars
                .get(idx + 1)
                .is_some_and(|(_, c)| c.is_ascii_alphanumeric() || *c == '-')
        {
            let start = byte_pos;
            idx += 1;
            while idx < chars.len() && !chars[idx].1.is_whitespace() {
                idx += 1;
            }
            let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
            spans.push(Span::styled(
                line[start..end].to_owned(),
                bg.fg(Color::Yellow),
            ));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit()
            && (idx == 0 || !chars[idx - 1].1.is_ascii_alphanumeric() && chars[idx - 1].1 != '_')
        {
            let start = byte_pos;
            idx += 1;
            while idx < chars.len()
                && (chars[idx].1.is_ascii_alphanumeric() || matches!(chars[idx].1, '.' | '_'))
            {
                idx += 1;
            }
            let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
            spans.push(Span::styled(
                line[start..end].to_owned(),
                bg.fg(Color::Magenta),
            ));
            continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = byte_pos;
            idx += 1;
            while idx < chars.len()
                && (chars[idx].1.is_ascii_alphanumeric() || matches!(chars[idx].1, '_' | '-'))
            {
                if chars[idx].1 == '-' && !is_shell {
                    break;
                }
                idx += 1;
            }
            let is_macro = !is_shell && chars.get(idx).is_some_and(|(_, c)| *c == '!');
            if is_macro {
                idx += 1;
            }
            let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
            let word = &line[start..end];
            let style = if is_macro {
                bg.fg(Color::Yellow)
            } else if is_keyword(word, is_shell) {
                bg.fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if !is_shell && word.starts_with(|c: char| c.is_ascii_uppercase()) {
                bg.fg(Color::Yellow)
            } else {
                bg
            };
            spans.push(Span::styled(word.to_owned(), style));
            continue;
        }

        // Plain punctuation / whitespace
        let start = byte_pos;
        idx += 1;
        let end = chars.get(idx).map_or(line.len(), |(b, _)| *b);
        spans.push(Span::styled(line[start..end].to_owned(), bg));
    }
    spans
}

fn is_keyword(word: &str, is_shell: bool) -> bool {
    if is_shell {
        matches!(
            word,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "in"
                | "do"
                | "done"
                | "while"
                | "case"
                | "esac"
                | "export"
                | "local"
                | "return"
                | "exit"
                | "set"
                | "cd"
                | "git"
                | "cargo"
                | "curl"
                | "sh"
                | "bash"
                | "sudo"
                | "echo"
                | "mkdir"
                | "rm"
                | "cp"
                | "mv"
                | "ls"
                | "cat"
                | "grep"
                | "chmod"
                | "tar"
                | "install"
                | "t3"
                | "tui3"
        )
    } else {
        matches!(
            word,
            "fn" | "let"
                | "mut"
                | "pub"
                | "struct"
                | "enum"
                | "impl"
                | "trait"
                | "match"
                | "if"
                | "else"
                | "for"
                | "in"
                | "while"
                | "loop"
                | "return"
                | "break"
                | "continue"
                | "async"
                | "await"
                | "use"
                | "mod"
                | "const"
                | "static"
                | "type"
                | "where"
                | "self"
                | "Self"
                | "true"
                | "false"
                | "null"
                | "None"
                | "Some"
                | "Ok"
                | "Err"
                | "function"
                | "class"
                | "import"
                | "from"
                | "export"
                | "default"
                | "def"
                | "var"
                | "new"
                | "try"
                | "catch"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_lists_links_inline_formatting_and_code_blocks() {
        let md = "# Title\n\n- **Bold** and *italic* with `code` and [README.md](file:///repo/README.md)\n- [x] Done task\n\n```bash\ngit tag v0.1.0 --annotate\n```\n";
        let lines = render(md, Color::Green, Color::White);
        let plain: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plain.contains("Title"));
        assert!(!plain.contains("# Title"));
        assert!(plain.contains("• Bold and italic with  code  and README.md"));
        assert!(!plain.contains("file:///repo/README.md"));
        assert!(plain.contains("☑ Done task"));
        assert!(plain.contains("┌─ bash "));
        assert!(plain.contains("┃ git tag v0.1.0 --annotate"));
        assert!(plain.contains("└─"));
        // Verify styling is applied to bold, link, and bash flag
        let all_spans: Vec<&Span<'_>> = lines.iter().flat_map(|l| l.spans.iter()).collect();
        assert!(all_spans.iter().any(|s| s.content == "Bold"
            && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(all_spans.iter().any(|s| s.content == "README.md"
            && s.style.add_modifier.contains(Modifier::UNDERLINED)));
        assert!(all_spans.iter().any(|s| s.content == "--annotate"
            && s.style.fg == Some(Color::Yellow)));
    }
}
