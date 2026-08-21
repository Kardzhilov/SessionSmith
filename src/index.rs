//! Per-campaign SQLite index of sessions and generated artifacts, enabling
//! cross-session search (`sessionsmith search`). The database is a single file
//! at `output/<slug>/index.sqlite`; nothing leaves the machine.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::CampaignConfig;

/// A single search result row.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session: String,
    pub kind: String,
    pub path: String,
    pub snippet: String,
}

fn db_path(campaign: &CampaignConfig) -> PathBuf {
    campaign.output_root().join("index.sqlite")
}

fn open(campaign: &CampaignConfig) -> Result<Connection> {
    let path = db_path(campaign);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn =
        Connection::open(&path).with_context(|| format!("opening index db {}", path.display()))?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS artifacts (
            id       INTEGER PRIMARY KEY,
            session  TEXT NOT NULL,
            kind     TEXT NOT NULL,
            path     TEXT NOT NULL,
            content  TEXT NOT NULL,
            updated  INTEGER NOT NULL,
            UNIQUE(session, kind)
         );
         CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session);",
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 2 {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS artifacts_fts USING fts5(
                session, kind, path UNINDEXED, content, tokenize = 'porter unicode61'
             );
             DELETE FROM artifacts_fts;
             INSERT INTO artifacts_fts(rowid, session, kind, path, content)
             SELECT id, session, kind, path, content FROM artifacts;
             PRAGMA user_version = 2;",
        )?;
    }
    Ok(())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Index (or re-index) every artifact file in a session's notes directory.
pub fn record_session(campaign: &CampaignConfig, stem: &str, notes_dir: &Path) -> Result<()> {
    let conn = open(campaign)?;
    record_into(&conn, stem, notes_dir)
}

fn record_into(conn: &Connection, stem: &str, notes_dir: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(notes_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "md" && ext != "json" {
            continue;
        }
        let kind = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        conn.execute(
            "INSERT INTO artifacts (session, kind, path, content, updated)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session, kind) DO UPDATE SET
                path = excluded.path,
                content = excluded.content,
                updated = excluded.updated",
            rusqlite::params![stem, kind, path.to_string_lossy(), content, now_secs()],
        )?;
        let row_id: i64 = conn.query_row(
            "SELECT id FROM artifacts WHERE session = ?1 AND kind = ?2",
            rusqlite::params![stem, kind],
            |row| row.get(0),
        )?;
        conn.execute("DELETE FROM artifacts_fts WHERE rowid = ?1", [row_id])?;
        conn.execute(
            "INSERT INTO artifacts_fts(rowid, session, kind, path, content) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![row_id, stem, kind, path.to_string_lossy(), content],
        )?;
    }
    Ok(())
}

/// Full-text-ish search across all indexed artifacts (case-insensitive LIKE).
pub fn search(campaign: &CampaignConfig, query: &str) -> Result<Vec<SearchHit>> {
    let path = db_path(campaign);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = open(campaign)?;
    search_conn(&conn, query)
}

fn search_conn(conn: &Connection, query: &str) -> Result<Vec<SearchHit>> {
    if query.contains(['\\', '%', '_']) {
        return search_like(conn, query);
    }
    match search_fts(conn, query) {
        Ok(hits) => Ok(hits),
        Err(error) => {
            crate::ui::warn(&format!(
                "FTS query unavailable ({error}); using literal search"
            ));
            search_like(conn, query)
        }
    }
}

fn fts_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| term.replace('"', "\"\""))
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn search_fts(conn: &Connection, query: &str) -> Result<Vec<SearchHit>> {
    let Some(query) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT session, kind, path,
                snippet(artifacts_fts, 3, '«', '»', ' … ', 12) AS snippet
         FROM artifacts_fts
         WHERE artifacts_fts MATCH ?1
         ORDER BY bm25(artifacts_fts)
         LIMIT 40",
    )?;
    let rows = stmt.query_map([query], |row| {
        Ok(SearchHit {
            session: row.get(0)?,
            kind: row.get(1)?,
            path: row.get(2)?,
            snippet: row.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn search_like(conn: &Connection, query: &str) -> Result<Vec<SearchHit>> {
    let like = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut stmt = conn.prepare(
        "SELECT session, kind, path, content FROM artifacts
         WHERE content LIKE ?1 ESCAPE '\\'
         ORDER BY session, kind",
    )?;
    let needle = query.to_lowercase();
    let rows = stmt.query_map([&like], |row| {
        let session: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let path: String = row.get(2)?;
        let content: String = row.get(3)?;
        Ok((session, kind, path, content))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (session, kind, path, content) = row?;
        let snippet = make_snippet(&content, &needle);
        hits.push(SearchHit {
            session,
            kind,
            path,
            snippet,
        });
    }
    Ok(hits)
}

/// Extract a short context window around the first match of `needle`.
fn make_snippet(content: &str, needle: &str) -> String {
    let lower = content.to_lowercase();
    let Some(pos) = lower.find(needle) else {
        return content.chars().take(120).collect();
    };
    let start = content[..pos]
        .char_indices()
        .rev()
        .nth(40)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let end_rel = pos + needle.len();
    let end = content[end_rel..]
        .char_indices()
        .nth(80)
        .map(|(i, _)| end_rel + i)
        .unwrap_or(content.len());
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.push_str(content[start..end].trim());
    if end < content.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_search_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("summary.md"),
            "The party found the cursed amulet in the crypt.",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("recap.md"),
            "A quiet evening at the tavern.",
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "should be ignored").unwrap();

        record_into(&conn, "session1", dir.path()).unwrap();

        // Case-insensitive match, ignores non md/json files.
        let hits = search_conn(&conn, "Cursed Amulet").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session, "session1");
        assert_eq!(hits[0].kind, "summary.md");
        assert!(hits[0]
            .snippet
            .replace(['«', '»'], "")
            .to_lowercase()
            .contains("cursed amulet"));

        // No match.
        assert!(search_conn(&conn, "dragon").unwrap().is_empty());

        // Re-recording upserts rather than duplicating.
        record_into(&conn, "session1", dir.path()).unwrap();
        assert_eq!(search_conn(&conn, "tavern").unwrap().len(), 1);
    }

    #[test]
    fn search_treats_backslashes_as_literals() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO artifacts (session, kind, path, content, updated) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["session1", "summary.md", "summary.md", "Found at C:\\notes", 0],
        )
        .unwrap();

        assert_eq!(search_conn(&conn, "C:\\notes").unwrap().len(), 1);
    }

    #[test]
    fn migrates_v1_database_into_fts_without_losing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE artifacts (
                id INTEGER PRIMARY KEY, session TEXT NOT NULL, kind TEXT NOT NULL,
                path TEXT NOT NULL, content TEXT NOT NULL, updated INTEGER NOT NULL,
                UNIQUE(session, kind)
             );
             INSERT INTO artifacts(session, kind, path, content, updated)
             VALUES ('session1', 'summary.md', 'summary.md', 'The basilisk is awake.', 0);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        init_schema(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert_eq!(search_conn(&conn, "basilisk").unwrap().len(), 1);
    }
}
