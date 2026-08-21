//! Minimal Markdown → `ratatui` renderer for the artifact/log preview. Handles
//! the constructs that appear in generated notes: ATX headings, bullet and
//! numbered lists, blockquotes, horizontal rules, fenced code blocks, and
//! inline **bold**, *italic* and `code` spans. Anything else renders as text.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::Theme;

/// Render markdown source `lines` into styled `ratatui` lines.
pub fn render_lines(lines: &[String], th: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(lines.len());
    let mut in_code = false;
    let mut source = lines.iter().peekable();

    while let Some(raw) = source.next() {
        let line = raw.as_str();
        let trimmed = line.trim_start();

        if is_table_line(trimmed) {
            let mut table = vec![line];
            while source
                .peek()
                .map(|next| is_table_line(next.trim_start()))
                .unwrap_or(false)
            {
                table.push(source.next().expect("peeked table row").as_str());
            }
            if let Some(rendered) = render_table(&table, th) {
                out.extend(rendered);
                continue;
            }
            for row in table {
                out.push(Line::from(inline(row, th, th.base())));
            }
            continue;
        }

        // Fenced code blocks.
        if trimmed.starts_with("```") {
            in_code = !in_code;
            out.push(Line::from(Span::styled(
                "─".repeat(3),
                Style::default().fg(th.muted),
            )));
            continue;
        }
        if in_code {
            out.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(th.accent),
            )));
            continue;
        }

        // Horizontal rule.
        if matches!(trimmed, "---" | "***" | "___") {
            out.push(Line::from(Span::styled(
                "──────────────────────────────",
                Style::default().fg(th.muted),
            )));
            continue;
        }

        // ATX headings.
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default().fg(th.primary).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(th.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }

        // Blockquote.
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let indent = line.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("▏ ", Style::default().fg(th.muted)),
            ];
            spans.extend(inline(
                rest,
                th,
                Style::default().fg(th.muted).add_modifier(Modifier::ITALIC),
            ));
            out.push(Line::from(spans));
            continue;
        }

        // Bullet list (-, *, +).
        if let Some(rest) = strip_bullet(trimmed) {
            let indent = line.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("• ", Style::default().fg(th.accent)),
            ];
            spans.extend(inline(rest, th, th.base()));
            out.push(Line::from(spans));
            continue;
        }

        // Numbered list.
        if let Some((num, rest)) = strip_ordered(trimmed) {
            let indent = line.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(format!("{num}. "), Style::default().fg(th.accent)),
            ];
            spans.extend(inline(rest, th, th.base()));
            out.push(Line::from(spans));
            continue;
        }

        // Plain paragraph line.
        out.push(Line::from(inline(line, th, th.base())));
    }

    out
}

fn is_table_line(line: &str) -> bool {
    line.starts_with('|') && line.ends_with('|')
}

fn render_table(rows: &[&str], th: &Theme) -> Option<Vec<Line<'static>>> {
    if rows.len() < 2 {
        return None;
    }
    let cells: Vec<Vec<String>> = rows.iter().map(|row| table_cells(row)).collect();
    let columns = cells.first()?.len();
    if columns == 0
        || cells.iter().any(|row| row.len() != columns)
        || !cells[1]
            .iter()
            .all(|cell| cell.trim_matches(':').chars().all(|c| c == '-') && cell.len() >= 3)
    {
        return None;
    }
    let mut widths = vec![0; columns];
    for row in cells
        .iter()
        .enumerate()
        .filter_map(|(index, row)| (index != 1).then_some(row))
    {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(crate::ui::visible_width(cell));
        }
    }
    let mut out = Vec::new();
    for (index, row) in cells.iter().enumerate() {
        let spans = if index == 1 {
            widths
                .iter()
                .enumerate()
                .flat_map(|(column, width)| {
                    let mut spans = vec![Span::styled(
                        "─".repeat(*width),
                        Style::default().fg(th.muted),
                    )];
                    if column + 1 < columns {
                        spans.push(Span::styled("─┼─", Style::default().fg(th.muted)));
                    }
                    spans
                })
                .collect()
        } else {
            let mut spans = Vec::new();
            for (column, cell) in row.iter().enumerate() {
                let style = if index == 0 {
                    th.base().add_modifier(Modifier::BOLD)
                } else {
                    th.base()
                };
                spans.push(Span::styled(cell.clone(), style));
                let padding = widths[column].saturating_sub(crate::ui::visible_width(cell));
                spans.push(Span::raw(" ".repeat(padding)));
                if column + 1 < columns {
                    spans.push(Span::styled(" │ ", Style::default().fg(th.muted)));
                }
            }
            spans
        };
        out.push(Line::from(spans));
    }
    Some(out)
}

fn table_cells(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for character in row.trim().trim_matches('|').chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '|' {
            cells.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    cells.push(current.trim().to_string());
    cells
}

fn strip_bullet(s: &str) -> Option<&str> {
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = s.strip_prefix(p) {
            return Some(rest);
        }
    }
    None
}

fn strip_ordered(s: &str) -> Option<(String, &str)> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &s[digits.len()..];
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    Some((digits, rest))
}

/// Parse inline `**bold**`, `*italic*`/`_italic_` and `` `code` `` spans.
fn inline(text: &str, th: &Theme, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    let flush = |buf: &mut String, spans: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), base));
        }
    };

    while i < chars.len() {
        // Links: [text](url). URLs are omitted because viewer links are
        // informational and full targets make generated notes hard to scan.
        if chars[i] == '[' {
            if let Some(label_end) = find_close(&chars, i + 1, "]") {
                if label_end + 1 < chars.len() && chars[label_end + 1] == '(' {
                    if let Some(url_end) = find_close(&chars, label_end + 2, ")") {
                        flush(&mut buf, &mut spans);
                        let label: String = chars[i + 1..label_end].iter().collect();
                        spans.push(Span::styled(
                            label,
                            Style::default()
                                .fg(th.primary)
                                .add_modifier(Modifier::UNDERLINED),
                        ));
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }
        // Bold: ** ... **
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find_close(&chars, i + 2, "**") {
                flush(&mut buf, &mut spans);
                let inner: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(inner, base.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
        }
        // Inline code: `...`
        if chars[i] == '`' {
            if let Some(end) = find_close(&chars, i + 1, "`") {
                flush(&mut buf, &mut spans);
                let inner: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(inner, Style::default().fg(th.success)));
                i = end + 1;
                continue;
            }
        }
        // Italic: * ... * or _ ... _
        if (chars[i] == '*' || chars[i] == '_') && i + 1 < chars.len() && chars[i + 1] != chars[i] {
            let marker = chars[i].to_string();
            if let Some(end) = find_close(&chars, i + 1, &marker) {
                flush(&mut buf, &mut spans);
                let inner: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(inner, base.add_modifier(Modifier::ITALIC)));
                i = end + 1;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut buf, &mut spans);
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

fn find_close(chars: &[char], from: usize, marker: &str) -> Option<usize> {
    let m: Vec<char> = marker.chars().collect();
    let mut i = from;
    while i + m.len() <= chars.len() {
        if chars[i..i + m.len()] == m[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_tables_render_without_urls_or_panics() {
        let theme = crate::tui::theme::builtins().remove(0);
        let lines = vec![
            "[guide](https://example.test) [0:12]".into(),
            "| Name | Value |".into(),
            "| --- | :---: |".into(),
            "| café | 2 |".into(),
            "".into(),
            "|||".into(),
            "[unclosed".into(),
        ];
        let rendered = render_lines(&lines, &theme);
        let text = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("guide"));
        assert!(!text.contains("https://example.test"));
        assert!(text.contains("│"));
    }
}
