//! Deterministic campaign-log assembly.
//!
//! The rolling campaign log is stored as an `## Ongoing Threads` section
//! followed by one section per session. Each session section is fenced by
//! HTML-comment markers carrying the session's **stem** as a stable key:
//!
//! ```text
//! <!-- ss:session id=DnD1 date=2026-01-01 -->
//! ## Session 2 — The Drowned Bell (2026-01-01)
//! ...body...
//! <!-- ss:end -->
//! ```
//!
//! Keying by stem means re-running the same recording *updates* its section in
//! place instead of appending a duplicate.

/// One session's entry in the log.
#[derive(Debug, Clone)]
pub struct SessionBlock {
    /// Session stem — the stable dedup key.
    pub id: String,
    /// `YYYY-MM-DD` (preserved across re-runs so ordering stays stable).
    pub date: String,
    /// Short session title.
    pub title: String,
    /// Markdown body (no header, no markers).
    pub body: String,
}

/// A parsed campaign log.
#[derive(Debug, Clone, Default)]
pub struct CampaignLog {
    /// Body of the `## Ongoing Threads` section (markdown, no header).
    pub threads: String,
    /// Session blocks in file order (newest first).
    pub blocks: Vec<SessionBlock>,
}

/// Whether `text` is in the marker-based v2 format.
pub fn is_v2(text: &str) -> bool {
    text.contains("<!-- ss:session")
}

/// Parse a v2 campaign log. Returns an empty log for empty/legacy input
/// (legacy is detected with [`is_v2`] by callers before choosing to migrate).
pub fn parse(text: &str) -> CampaignLog {
    let mut log = CampaignLog::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    // Threads: lines after "## Ongoing Threads" until the first session marker
    // or another top-level heading.
    while i < lines.len() {
        if lines[i].trim_start().starts_with("## Ongoing Threads") {
            i += 1;
            let mut body = Vec::new();
            while i < lines.len()
                && !lines[i].trim_start().starts_with("<!-- ss:session")
            {
                body.push(lines[i]);
                i += 1;
            }
            log.threads = body.join("\n").trim().to_string();
            break;
        }
        i += 1;
    }

    // Session blocks.
    for idx in 0..lines.len() {
        let line = lines[idx].trim();
        if let Some(rest) = line
            .strip_prefix("<!-- ss:session")
            .and_then(|s| s.strip_suffix("-->"))
        {
            let (id, date) = parse_marker_attrs(rest);
            // Collect body until the matching end marker.
            let mut title = String::new();
            let mut body = Vec::new();
            let mut j = idx + 1;
            let mut first = true;
            while j < lines.len() && !lines[j].trim().starts_with("<!-- ss:end") {
                if first && lines[j].trim_start().starts_with("## ") {
                    title = parse_title(lines[j].trim_start());
                    first = false;
                } else {
                    body.push(lines[j]);
                    first = false;
                }
                j += 1;
            }
            log.blocks.push(SessionBlock {
                id,
                date,
                title,
                body: body.join("\n").trim().to_string(),
            });
        }
    }
    log
}

fn parse_marker_attrs(s: &str) -> (String, String) {
    let mut id = String::new();
    let mut date = String::new();
    for tok in s.split_whitespace() {
        if let Some(v) = tok.strip_prefix("id=") {
            id = v.to_string();
        } else if let Some(v) = tok.strip_prefix("date=") {
            date = v.to_string();
        }
    }
    (id, date)
}

/// Extract the title from a `## Session N — Title (date)` header line.
fn parse_title(header: &str) -> String {
    let h = header.trim_start_matches('#').trim();
    // Prefer the text after an em/en dash.
    let after = h.split(['—', '-']).nth(1).map(|s| s.trim()).unwrap_or(h);
    // Drop a trailing "(date)".
    let title = if let Some(pos) = after.rfind('(') {
        after[..pos].trim()
    } else {
        after
    };
    title.to_string()
}

impl CampaignLog {
    /// Insert or update the block for `id`. Existing blocks keep their original
    /// date (so re-runs don't reorder the log); new blocks go to the front.
    pub fn upsert(&mut self, id: &str, date: &str, title: String, body: String) {
        if let Some(b) = self.blocks.iter_mut().find(|b| b.id == id) {
            b.title = title;
            b.body = body;
        } else {
            self.blocks.insert(
                0,
                SessionBlock {
                    id: id.to_string(),
                    date: date.to_string(),
                    title,
                    body,
                },
            );
        }
    }

    /// The body of the block for `id`, if present.
    pub fn block_body(&self, id: &str) -> Option<&str> {
        self.blocks.iter().find(|b| b.id == id).map(|b| b.body.as_str())
    }

    /// Render the full markdown document.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# Campaign Log\n\n");
        out.push_str("## Ongoing Threads\n\n");
        if self.threads.trim().is_empty() {
            out.push_str("_No open threads yet._\n");
        } else {
            out.push_str(self.threads.trim());
            out.push('\n');
        }
        let n = self.blocks.len();
        for (i, b) in self.blocks.iter().enumerate() {
            let num = n - i; // newest first → highest number on top
            out.push('\n');
            out.push_str(&format!("<!-- ss:session id={} date={} -->\n", b.id, b.date));
            out.push_str(&format!("## Session {num} — {} ({})\n\n", b.title, b.date));
            out.push_str(b.body.trim());
            out.push_str("\n<!-- ss:end -->\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_dedups_by_stem() {
        let mut log = CampaignLog::default();
        log.upsert("DnD1", "2026-01-01", "First".into(), "body a".into());
        log.upsert("DnD2", "2026-01-08", "Second".into(), "body b".into());
        // Re-run of DnD1 must not add a new block.
        log.upsert("DnD1", "2026-02-01", "First redux".into(), "body a2".into());
        assert_eq!(log.blocks.len(), 2);
        let b = log.blocks.iter().find(|b| b.id == "DnD1").unwrap();
        assert_eq!(b.title, "First redux");
        assert_eq!(b.date, "2026-01-01"); // date preserved
    }

    #[test]
    fn round_trip_parse_render() {
        let mut log = CampaignLog {
            threads: "- Find the bell".into(),
            ..Default::default()
        };
        log.upsert("DnD1", "2026-01-01", "The Bell".into(), "They found it.".into());
        let text = log.render();
        assert!(is_v2(&text));
        let parsed = parse(&text);
        assert_eq!(parsed.blocks.len(), 1);
        assert_eq!(parsed.blocks[0].id, "DnD1");
        assert_eq!(parsed.blocks[0].title, "The Bell");
        assert!(parsed.threads.contains("Find the bell"));
    }
}
