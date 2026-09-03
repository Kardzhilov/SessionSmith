//! Per-campaign SQLite index of sessions and generated artifacts, enabling
//! cross-session search (`sessionsmith search`). The database is a single file
//! in the user cache directory; nothing leaves the machine.

use anyhow::{bail, Context, Result};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    let key = campaign_cache_key(campaign);
    dirs::cache_dir()
        .unwrap_or_else(|| campaign.output_root())
        .join("sessionsmith")
        .join("indexes")
        .join(key)
        .join("index.sqlite")
}

pub fn campaign_index_path_at(cache_root: &Path, campaign: &CampaignConfig) -> PathBuf {
    cache_root
        .join("sessionsmith")
        .join("indexes")
        .join(campaign_cache_key(campaign))
        .join("index.sqlite")
}

fn campaign_cache_key(campaign: &CampaignConfig) -> String {
    let source = campaign
        .source_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                campaign.campaign.name,
                campaign.output_root().display()
            )
        });
    format!("{}-{:016x}", campaign.slug(), fnv1a64(source.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn legacy_db_path(campaign: &CampaignConfig) -> PathBuf {
    campaign.output_root().join("index.sqlite")
}

fn open(campaign: &CampaignConfig) -> Result<Connection> {
    let path = db_path(campaign);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let legacy = legacy_db_path(campaign);
    if !path.exists() && legacy.exists() {
        migrate_legacy_db(&legacy, &path)?;
        crate::ui::info(&format!("migrated search index to {}", path.display()));
    }
    let conn =
        Connection::open(&path).with_context(|| format!("opening index db {}", path.display()))?;
    init_schema(&conn)?;
    Ok(conn)
}

fn migrate_legacy_db(legacy: &Path, destination: &Path) -> Result<()> {
    migrate_legacy_db_with_validator(legacy, destination, validate_migrated_db)
}

fn migrate_legacy_db_with_validator<F>(legacy: &Path, destination: &Path, validate: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if destination.exists() {
        bail!(
            "index migration destination already exists: {}",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("index migration destination has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating index directory {}", parent.display()))?;
    let temporary = migration_temporary_path(destination);
    if temporary.exists() {
        bail!(
            "index migration temporary destination already exists: {}",
            temporary.display()
        );
    }

    let result = (|| -> Result<()> {
        let source = Connection::open(legacy)
            .with_context(|| format!("opening legacy index {}", legacy.display()))?;
        let checkpoint_busy: i64 =
            source.query_row("PRAGMA wal_checkpoint(FULL)", [], |row| row.get(0))?;
        if checkpoint_busy != 0 {
            bail!(
                "legacy index WAL checkpoint is busy; leaving {} in place",
                legacy.display()
            );
        }

        let mut copied = Connection::open(&temporary).with_context(|| {
            format!("creating temporary migrated index {}", temporary.display())
        })?;
        {
            let backup = Backup::new(&source, &mut copied)?;
            backup.run_to_completion(128, Duration::from_millis(10), None)?;
        }
        copied
            .close()
            .map_err(|(_, error)| error)
            .with_context(|| format!("closing temporary index {}", temporary.display()))?;
        validate(&temporary)?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .with_context(|| format!("opening migrated index for sync {}", temporary.display()))?
            .sync_all()
            .with_context(|| format!("syncing migrated index {}", temporary.display()))?;
        std::fs::rename(&temporary, destination)
            .with_context(|| format!("installing migrated index {}", destination.display()))?;
        sync_directory(parent)?;
        Ok(())
    })();

    if let Err(error) = result {
        if temporary.is_file() {
            let _ = std::fs::remove_file(&temporary);
        }
        return Err(error).with_context(|| format!("migrating legacy index {}", legacy.display()));
    }

    for path in [
        legacy.to_path_buf(),
        sidecar_path(legacy, "-wal"),
        sidecar_path(legacy, "-shm"),
    ] {
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                crate::ui::warn(&format!(
                    "migrated search index but could not remove legacy file {}: {error}",
                    path.display()
                ));
            }
        }
    }
    let _ = legacy.parent().map(sync_directory).transpose();
    Ok(())
}

fn validate_migrated_db(path: &Path) -> Result<()> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening migrated index for validation {}", path.display()))?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        bail!("migrated index failed integrity validation: {integrity}");
    }
    let _: i64 = connection.query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
    Ok(())
}

fn migration_temporary_path(destination: &Path) -> PathBuf {
    sidecar_path(destination, ".migration-tmp")
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    std::fs::File::open(path)
        .with_context(|| format!("opening directory for sync {}", path.display()))?
        .sync_all()
        .with_context(|| format!("syncing directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_: &Path) -> Result<()> {
    Ok(())
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

/// Copy a campaign index to its new identity while preserving the source.
/// Artifact paths beneath the old output root are rewritten to the new root.
pub fn migrate_campaign_index_at(
    cache_root: &Path,
    old_campaign: &CampaignConfig,
    new_campaign: &CampaignConfig,
    old_output_root: &Path,
    new_output_root: &Path,
) -> Result<bool> {
    let old_path = campaign_index_path_at(cache_root, old_campaign);
    let new_path = campaign_index_path_at(cache_root, new_campaign);
    if old_path == new_path || !old_path.is_file() {
        return Ok(false);
    }
    if new_path.exists() {
        bail!(
            "campaign search index already exists: {}; refusing to overwrite it",
            new_path.display()
        );
    }
    let parent = new_path
        .parent()
        .context("campaign search index path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating index directory {}", parent.display()))?;

    let result = (|| -> Result<()> {
        let source = Connection::open(&old_path)
            .with_context(|| format!("opening source index {}", old_path.display()))?;
        init_schema(&source)?;
        let mut destination = Connection::open(&new_path)
            .with_context(|| format!("creating destination index {}", new_path.display()))?;
        init_schema(&destination)?;

        let rows = {
            let mut statement = source.prepare(
                "SELECT id, session, kind, path, content, updated FROM artifacts ORDER BY id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        let transaction = destination.transaction()?;
        for (id, session, kind, path, content, updated) in &rows {
            let path = Path::new(path)
                .strip_prefix(old_output_root)
                .map(|relative| new_output_root.join(relative))
                .unwrap_or_else(|_| PathBuf::from(path));
            transaction.execute(
                "INSERT INTO artifacts (id, session, kind, path, content, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, session, kind, path.to_string_lossy(), content, updated],
            )?;
        }
        transaction.execute("DELETE FROM artifacts_fts", [])?;
        transaction.execute(
            "INSERT INTO artifacts_fts(rowid, session, kind, path, content)
             SELECT id, session, kind, path, content FROM artifacts",
            [],
        )?;
        let copied: i64 =
            transaction.query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
        if copied != rows.len() as i64 {
            bail!(
                "campaign search index migration copied {copied} of {} rows",
                rows.len()
            );
        }
        transaction.commit()?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(parent);
        return Err(error);
    }
    Ok(true)
}

pub fn remove_campaign_index_at(cache_root: &Path, campaign: &CampaignConfig) -> Result<()> {
    let path = campaign_index_path_at(cache_root, campaign);
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.exists() {
        std::fs::remove_dir_all(parent)
            .with_context(|| format!("removing campaign index {}", parent.display()))?;
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
    let transcript = campaign.transcripts_dir().join(format!("{stem}.txt"));
    record_session_at(campaign, stem, notes_dir, &transcript)
}

pub fn record_session_at(
    campaign: &CampaignConfig,
    stem: &str,
    notes_dir: &Path,
    transcript: &Path,
) -> Result<()> {
    let conn = open(campaign)?;
    record_into_with_transcript(&conn, stem, notes_dir, Some(transcript))
}

pub fn delete_session(campaign: &CampaignConfig, stem: &str) -> Result<()> {
    let mut conn = open(campaign)?;
    delete_session_from(&mut conn, stem)
}

fn delete_session_from(conn: &mut Connection, stem: &str) -> Result<()> {
    let transaction = conn.transaction()?;
    let ids = {
        let mut statement = transaction.prepare("SELECT id FROM artifacts WHERE session = ?1")?;
        let ids = statement
            .query_map([stem], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    for id in ids {
        transaction.execute("DELETE FROM artifacts_fts WHERE rowid = ?1", [id])?;
    }
    transaction.execute("DELETE FROM artifacts WHERE session = ?1", [stem])?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
fn record_into(conn: &Connection, stem: &str, notes_dir: &Path) -> Result<()> {
    record_into_with_transcript(conn, stem, notes_dir, None)
}

fn record_into_with_transcript(
    conn: &Connection,
    stem: &str,
    notes_dir: &Path,
    transcript: Option<&Path>,
) -> Result<()> {
    let mut indexed_kinds = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(notes_dir) {
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
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            record_document(conn, stem, &kind, &path)?;
            indexed_kinds.insert(kind);
        }
    }
    if let Some(path) = transcript.filter(|path| path.is_file()) {
        record_document(conn, stem, "transcript", path)?;
        indexed_kinds.insert("transcript".into());
    }
    remove_stale_session_records(conn, stem, &indexed_kinds)?;
    Ok(())
}

fn record_document(conn: &Connection, stem: &str, kind: &str, path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
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
    Ok(())
}

fn remove_stale_session_records(
    conn: &Connection,
    stem: &str,
    indexed_kinds: &BTreeSet<String>,
) -> Result<()> {
    let mut statement = conn.prepare("SELECT id, kind FROM artifacts WHERE session = ?1")?;
    let rows = statement.query_map([stem], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let stale = rows
        .filter_map(|row| row.ok())
        .filter(|(_, kind)| !indexed_kinds.contains(kind))
        .collect::<Vec<_>>();
    drop(statement);

    for (id, _) in stale {
        conn.execute("DELETE FROM artifacts_fts WHERE rowid = ?1", [id])?;
        conn.execute("DELETE FROM artifacts WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Full-text-ish search across all indexed artifacts (case-insensitive LIKE).
pub fn search(campaign: &CampaignConfig, query: &str) -> Result<Vec<SearchHit>> {
    search_filtered(campaign, query, &[])
}

pub fn search_filtered(
    campaign: &CampaignConfig,
    query: &str,
    kinds: &[String],
) -> Result<Vec<SearchHit>> {
    let path = db_path(campaign);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = open(campaign)?;
    search_conn_filtered(&conn, query, kinds)
}

#[cfg(test)]
fn search_conn(conn: &Connection, query: &str) -> Result<Vec<SearchHit>> {
    search_conn_filtered(conn, query, &[])
}

fn search_conn_filtered(
    conn: &Connection,
    query: &str,
    kinds: &[String],
) -> Result<Vec<SearchHit>> {
    if query.contains(['\\', '%', '_']) {
        return filter_hits(search_like(conn, query)?, kinds);
    }
    match search_fts(conn, query) {
        Ok(hits) => filter_hits(hits, kinds),
        Err(error) => {
            crate::ui::warn(&format!(
                "FTS query unavailable ({error}); using literal search"
            ));
            filter_hits(search_like(conn, query)?, kinds)
        }
    }
}

fn filter_hits(mut hits: Vec<SearchHit>, kinds: &[String]) -> Result<Vec<SearchHit>> {
    if !kinds.is_empty() {
        hits.retain(|hit| kinds.iter().any(|kind| kind == &hit.kind));
    }
    hits.truncate(40);
    Ok(hits)
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
         LIMIT 200",
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
        let snippet = highlight_markers(&make_snippet(&content, &needle), query);
        hits.push(SearchHit {
            session,
            kind,
            path,
            snippet,
        });
    }
    Ok(hits)
}

/// Mark every case-insensitive query-word match with `«` and `»` for clients
/// that can render highlighted snippets. Existing FTS5 markers are preserved.
pub fn highlight_markers(snippet: &str, query: &str) -> String {
    if snippet.contains('«') || query.trim().is_empty() {
        return snippet.to_string();
    }
    let chars: Vec<char> = snippet.chars().collect();
    let terms: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|term| term.chars().flat_map(char::to_lowercase).collect())
        .filter(|term: &Vec<char>| !term.is_empty())
        .collect();
    let mut matched = vec![false; chars.len()];
    for term in terms {
        if term.len() > chars.len() {
            continue;
        }
        for start in 0..=chars.len() - term.len() {
            if chars[start..start + term.len()]
                .iter()
                .flat_map(|character| character.to_lowercase())
                .eq(term.iter().copied())
            {
                matched[start..start + term.len()].fill(true);
            }
        }
    }
    let mut output = String::with_capacity(snippet.len());
    let mut in_match = false;
    for (character, is_match) in chars.into_iter().zip(matched) {
        if is_match != in_match {
            output.push(if is_match { '«' } else { '»' });
            in_match = is_match;
        }
        output.push(character);
    }
    if in_match {
        output.push('»');
    }
    output
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
    fn deleting_session_removes_base_and_fts_results() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("summary.md"), "The moonwell opens.").unwrap();
        record_into(&conn, "old-session", directory.path()).unwrap();
        assert_eq!(search_conn(&conn, "moonwell").unwrap().len(), 1);

        delete_session_from(&mut conn, "old-session").unwrap();

        assert!(search_conn(&conn, "moonwell").unwrap().is_empty());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM artifacts_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn transcript_content_is_indexed_filtered_and_removed_when_missing() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let notes = tempfile::tempdir().unwrap();
        let transcripts = tempfile::tempdir().unwrap();
        let transcript = transcripts.path().join("session1.txt");
        std::fs::write(
            notes.path().join("summary.md"),
            "The raven reaches the tower.",
        )
        .unwrap();
        std::fs::write(&transcript, "SPEAKER_00: The moonwell opens below us.").unwrap();

        record_into_with_transcript(&conn, "session1", notes.path(), Some(&transcript)).unwrap();
        let transcript_kinds = vec!["transcript".to_string()];
        let summary_kinds = vec!["summary.md".to_string()];
        let transcript_hits = search_conn_filtered(&conn, "moonwell", &transcript_kinds).unwrap();
        assert_eq!(transcript_hits.len(), 1);
        assert_eq!(transcript_hits[0].kind, "transcript");
        assert!(search_conn_filtered(&conn, "moonwell", &summary_kinds)
            .unwrap()
            .is_empty());

        std::fs::remove_file(&transcript).unwrap();
        record_into_with_transcript(&conn, "session1", notes.path(), Some(&transcript)).unwrap();
        assert!(search_conn(&conn, "moonwell").unwrap().is_empty());
    }

    #[test]
    fn reindexing_a_session_removes_deleted_candidate_artifacts() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("summary.md");
        let candidate = dir.path().join("summary.candidate.md");
        std::fs::write(&current, "The original record.").unwrap();
        std::fs::write(&candidate, "A temporary candidate marker.").unwrap();
        record_into(&conn, "session1", dir.path()).unwrap();
        assert!(search_conn(&conn, "temporary")
            .unwrap()
            .iter()
            .any(|hit| { hit.kind == "summary.candidate.md" }));

        std::fs::remove_file(&candidate).unwrap();
        std::fs::write(&current, "The promoted candidate marker.").unwrap();
        record_into(&conn, "session1", dir.path()).unwrap();

        assert!(search_conn(&conn, "temporary").unwrap().is_empty());
        assert_eq!(
            search_conn(&conn, "promoted")
                .unwrap()
                .first()
                .map(|hit| hit.kind.as_str()),
            Some("summary.md")
        );
    }

    #[test]
    fn campaign_cache_keys_do_not_collide_for_same_named_campaigns() {
        let mut first = CampaignConfig::default();
        first.campaign.name = "Shared Name".into();
        first.source_path = Some(PathBuf::from("/tmp/a/campaign.toml"));
        let mut second = first.clone();
        second.source_path = Some(PathBuf::from("/tmp/b/campaign.toml"));
        assert_ne!(campaign_cache_key(&first), campaign_cache_key(&second));
    }

    #[test]
    fn campaign_index_migration_rewrites_paths_and_preserves_fts() {
        let temp = tempfile::tempdir().unwrap();
        let old_output = temp.path().join("output/old-name");
        let new_output = temp.path().join("output/new-name");
        let old_notes = old_output.join("notes/session1");
        std::fs::create_dir_all(&old_notes).unwrap();
        std::fs::write(old_notes.join("summary.md"), "The migrated moonwell opens.").unwrap();

        let mut old_campaign = CampaignConfig::default();
        old_campaign.campaign.name = "Old Name".into();
        old_campaign.source_path = Some(temp.path().join("campaigns/old-name.toml"));
        let mut new_campaign = old_campaign.clone();
        new_campaign.campaign.name = "New Name".into();
        new_campaign.source_path = Some(temp.path().join("campaigns/new-name.toml"));

        let old_path = campaign_index_path_at(temp.path(), &old_campaign);
        std::fs::create_dir_all(old_path.parent().unwrap()).unwrap();
        let old_connection = Connection::open(&old_path).unwrap();
        init_schema(&old_connection).unwrap();
        record_into(&old_connection, "session1", &old_notes).unwrap();
        drop(old_connection);

        assert!(migrate_campaign_index_at(
            temp.path(),
            &old_campaign,
            &new_campaign,
            &old_output,
            &new_output,
        )
        .unwrap());

        let new_path = campaign_index_path_at(temp.path(), &new_campaign);
        let new_connection = Connection::open(new_path).unwrap();
        let hits = search_conn(&new_connection, "migrated moonwell").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            PathBuf::from(&hits[0].path),
            new_output.join("notes/session1/summary.md")
        );
        assert!(old_path.exists());
        assert_eq!(
            search_conn(&Connection::open(old_path).unwrap(), "migrated moonwell")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn campaign_index_migration_refuses_destination_collision() {
        let temp = tempfile::tempdir().unwrap();
        let mut old_campaign = CampaignConfig::default();
        old_campaign.campaign.name = "Old Name".into();
        old_campaign.source_path = Some(temp.path().join("old.toml"));
        let mut new_campaign = old_campaign.clone();
        new_campaign.campaign.name = "New Name".into();
        new_campaign.source_path = Some(temp.path().join("new.toml"));
        for campaign in [&old_campaign, &new_campaign] {
            let path = campaign_index_path_at(temp.path(), campaign);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let connection = Connection::open(path).unwrap();
            init_schema(&connection).unwrap();
        }

        let error = migrate_campaign_index_at(
            temp.path(),
            &old_campaign,
            &new_campaign,
            temp.path(),
            temp.path(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert!(campaign_index_path_at(temp.path(), &old_campaign).exists());
    }

    #[test]
    fn legacy_database_migration_preserves_searchable_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("output/index.sqlite");
        let destination = temp.path().join("cache/index.sqlite");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let conn = Connection::open(&legacy).unwrap();
        init_schema(&conn).unwrap();
        let notes = temp.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        std::fs::write(notes.join("summary.md"), "The legacy basilisk is awake.").unwrap();
        record_into(&conn, "session1", &notes).unwrap();
        drop(conn);

        migrate_legacy_db(&legacy, &destination).unwrap();
        assert!(!legacy.exists());
        assert!(destination.exists());
        let migrated = Connection::open(destination).unwrap();
        assert_eq!(search_conn(&migrated, "legacy basilisk").unwrap().len(), 1);
    }

    #[test]
    fn legacy_database_validation_failure_leaves_source_usable() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("output/index.sqlite");
        let destination = temp.path().join("cache/index.sqlite");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let connection = Connection::open(&legacy).unwrap();
        init_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO artifacts (session, kind, path, content, updated)
                 VALUES ('session1', 'summary.md', 'summary.md', 'validation sentinel', 0)",
                [],
            )
            .unwrap();
        drop(connection);

        let error = migrate_legacy_db_with_validator(&legacy, &destination, |_| {
            bail!("forced validation failure")
        })
        .unwrap_err();

        assert!(error.to_string().contains("migrating legacy index"));
        assert!(legacy.exists());
        assert!(!destination.exists());
        assert!(!migration_temporary_path(&destination).exists());
        let source = Connection::open(&legacy).unwrap();
        let count: i64 = source
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn legacy_database_migration_captures_wal_backed_search_data() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("output/index.sqlite");
        let destination = temp.path().join("cache/index.sqlite");
        let notes = temp.path().join("notes");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::create_dir(&notes).unwrap();
        std::fs::write(notes.join("summary.md"), "WAL-backed searchable sentinel.").unwrap();
        let connection = Connection::open(&legacy).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        init_schema(&connection).unwrap();
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        record_into(&connection, "session1", &notes).unwrap();
        let wal = sidecar_path(&legacy, "-wal");
        assert!(wal.metadata().unwrap().len() > 0);

        migrate_legacy_db(&legacy, &destination).unwrap();
        drop(connection);

        let migrated = Connection::open(destination).unwrap();
        assert_eq!(
            search_conn(&migrated, "searchable sentinel").unwrap().len(),
            1
        );
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
    fn highlight_markers_are_unicode_safe_and_cover_every_term() {
        assert_eq!(
            highlight_markers("The cafe cafe has café", "cafe CAFÉ"),
            "The «cafe» «cafe» has «café»"
        );
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
