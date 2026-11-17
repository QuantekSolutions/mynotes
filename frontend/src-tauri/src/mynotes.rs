//! MyNotes additions on top of Meetily: user notes captured during a meeting,
//! notes-aware summaries, and Markdown export.
//!
//! Notes live in the `meeting_notes` table (migration 20251223000000), keyed by
//! meeting id. Exports are assembled from what is stored locally — nothing is
//! generated here, so the export never contains content the user or the
//! summary pipeline did not produce.

use crate::api::MeetingDetails;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::state::AppState;
use chrono::Utc;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub async fn load_notes(pool: &SqlitePool, meeting_id: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT notes_markdown FROM meeting_notes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(md,)| md).filter(|md| !md.trim().is_empty()))
}

pub async fn store_notes(pool: &SqlitePool, meeting_id: &str, notes_markdown: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO meeting_notes (meeting_id, notes_markdown, notes_json, created_at, updated_at)
         VALUES (?, ?, NULL, ?, ?)
         ON CONFLICT(meeting_id) DO UPDATE SET notes_markdown = excluded.notes_markdown,
                                               updated_at = excluded.updated_at",
    )
    .bind(meeting_id)
    .bind(notes_markdown)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns the summary Markdown stored for a meeting (generated or user-edited).
async fn load_summary_markdown(pool: &SqlitePool, meeting_id: &str) -> Option<String> {
    let process = SummaryProcessesRepository::get_summary_data(pool, meeting_id).await.ok()??;
    let raw = process.result?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("markdown")
        .and_then(|m| m.as_str())
        .map(str::to_string)
        .filter(|m| !m.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Pure helpers (unit tested)
// ---------------------------------------------------------------------------

/// Formats a recording-relative offset in seconds as `[mm:ss]` / `[h:mm:ss]`.
pub fn format_offset(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("[{h}:{m:02}:{s:02}]")
    } else {
        format!("[{m:02}:{s:02}]")
    }
}

/// Assembles the export document. Sections with no stored content are
/// labelled as such rather than filled in.
pub fn build_meeting_markdown(
    meeting: &MeetingDetails,
    notes: Option<&str>,
    summary: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", meeting.title.trim()));
    out.push_str(&format!("- **Date:** {}\n", meeting.created_at));
    out.push_str(&format!("- **Meeting ID:** {}\n\n", meeting.id));

    out.push_str("## My Notes\n\n");
    match notes {
        Some(n) if !n.trim().is_empty() => out.push_str(&format!("{}\n\n", n.trim())),
        _ => out.push_str("_No notes were taken._\n\n"),
    }

    out.push_str("## Summary\n\n");
    match summary {
        Some(s) if !s.trim().is_empty() => out.push_str(&format!("{}\n\n", s.trim())),
        _ => out.push_str("_No summary has been generated yet._\n\n"),
    }

    out.push_str("## Transcript\n\n");
    if meeting.transcripts.is_empty() {
        out.push_str("_No transcript was captured._\n");
    } else {
        for t in &meeting.transcripts {
            let stamp = match t.audio_start_time {
                Some(start) => format_offset(start),
                None => format!("[{}]", t.timestamp),
            };
            out.push_str(&format!("{} {}\n\n", stamp, t.text.trim()));
        }
    }
    out
}

/// Makes a meeting title safe to use as a file name on every platform.
pub fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let limited: String = trimmed.chars().take(80).collect();
    if limited.is_empty() { "Meeting".to_string() } else { limited }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn mynotes_get_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<String>, String> {
    load_notes(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load notes: {e}"))
}

#[tauri::command]
pub async fn mynotes_save_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    notes_markdown: String,
) -> Result<(), String> {
    store_notes(state.db_manager.pool(), &meeting_id, &notes_markdown)
        .await
        .map_err(|e| {
            log::error!("Failed to save notes for {meeting_id}: {e}");
            format!("Failed to save notes: {e}")
        })?;
    log::info!("Saved notes for meeting {meeting_id} ({} chars)", notes_markdown.len());
    Ok(())
}

async fn meeting_markdown(pool: &SqlitePool, meeting_id: &str) -> Result<(MeetingDetails, String), String> {
    let meeting = MeetingsRepository::get_meeting(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {e}"))?
        .ok_or_else(|| format!("Meeting not found: {meeting_id}"))?;
    let notes = load_notes(pool, meeting_id).await.map_err(|e| format!("Failed to load notes: {e}"))?;
    let summary = load_summary_markdown(pool, meeting_id).await;
    let md = build_meeting_markdown(&meeting, notes.as_deref(), summary.as_deref());
    Ok((meeting, md))
}

#[cfg(test)]
pub(crate) async fn meeting_markdown_for_test(pool: &SqlitePool, meeting_id: &str) -> Result<String, String> {
    meeting_markdown(pool, meeting_id).await.map(|(_, md)| md)
}

/// Returns the full Markdown document for a meeting (used for copy / preview).
#[tauri::command]
pub async fn mynotes_meeting_markdown(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<String, String> {
    meeting_markdown(state.db_manager.pool(), &meeting_id).await.map(|(_, md)| md)
}

/// Writes the meeting as Markdown. If `path` is omitted, saves to
/// `~/Documents/MyNotes/<date> <title>.md`. Returns the written path.
#[tauri::command]
pub async fn mynotes_export_markdown<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    path: Option<String>,
    reveal: Option<bool>,
) -> Result<String, String> {
    let (meeting, md) = meeting_markdown(state.db_manager.pool(), &meeting_id).await?;

    let target: PathBuf = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => {
            let dir = app
                .path()
                .document_dir()
                .map_err(|e| format!("Could not resolve Documents folder: {e}"))?
                .join("MyNotes");
            let date: String = meeting.created_at.chars().take(10).collect();
            dir.join(format!("{} {}.md", date, sanitize_filename(&meeting.title)))
        }
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Could not create export folder: {e}"))?;
    }
    std::fs::write(&target, md).map_err(|e| format!("Could not write export: {e}"))?;
    log::info!("Exported meeting {meeting_id} to Markdown");
    if reveal.unwrap_or(false) {
        let to_reveal = target.clone();
        // Opening Finder can block; keep it off the async worker. Failure is non-fatal.
        let _ = tauri::async_runtime::spawn_blocking(move || reveal_in_file_manager(&to_reveal)).await;
    }
    Ok(target.to_string_lossy().to_string())
}

fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg("-R").arg(path).status().map(|_| ());
    #[cfg(not(target_os = "macos"))]
    let result = match path.parent() {
        Some(dir) => open::that(dir),
        None => Ok(()),
    };
    if let Err(e) = result {
        log::warn!("Could not reveal export in file manager: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MeetingTranscript;

    fn meeting(transcripts: Vec<MeetingTranscript>) -> MeetingDetails {
        MeetingDetails {
            id: "meeting-1".into(),
            title: "Weekly sync".into(),
            created_at: "2026-11-17T10:00:00+00:00".into(),
            updated_at: "2026-11-17T10:30:00+00:00".into(),
            transcripts,
        }
    }

    fn line(text: &str, start: Option<f64>) -> MeetingTranscript {
        MeetingTranscript {
            id: "t".into(),
            text: text.into(),
            timestamp: "10:00:05".into(),
            audio_start_time: start,
            audio_end_time: None,
            duration: None,
        }
    }

    #[test]
    fn offsets_format_as_clock() {
        assert_eq!(format_offset(5.4), "[00:05]");
        assert_eq!(format_offset(125.0), "[02:05]");
        assert_eq!(format_offset(3725.0), "[1:02:05]");
        assert_eq!(format_offset(-3.0), "[00:00]");
    }

    #[test]
    fn export_includes_all_sections_in_order() {
        let m = meeting(vec![line("We ship Friday.", Some(65.0)), line("Agreed.", None)]);
        let md = build_meeting_markdown(&m, Some("- ask about budget"), Some("## Decisions\n- Ship Friday"));
        let notes = md.find("## My Notes").unwrap();
        let summary = md.find("## Summary").unwrap();
        let transcript = md.find("## Transcript").unwrap();
        assert!(md.starts_with("# Weekly sync\n"));
        assert!(notes < summary && summary < transcript);
        assert!(md.contains("- ask about budget"));
        assert!(md.contains("[01:05] We ship Friday."));
        assert!(md.contains("[10:00:05] Agreed."));
    }

    #[test]
    fn export_marks_missing_content_instead_of_inventing_it() {
        let md = build_meeting_markdown(&meeting(vec![]), None, Some("   "));
        assert!(md.contains("_No notes were taken._"));
        assert!(md.contains("_No summary has been generated yet._"));
        assert!(md.contains("_No transcript was captured._"));
    }

    #[test]
    fn filenames_are_sanitized() {
        assert_eq!(sanitize_filename("Q4: plan / review?"), "Q4- plan - review-");
        assert_eq!(sanitize_filename("  ..  "), "Meeting");
        assert_eq!(sanitize_filename(&"x".repeat(200)).chars().count(), 80);
    }
}
