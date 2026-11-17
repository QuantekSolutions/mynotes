use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use log::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting with id: {}", meeting_id);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!("Failed to save transcript segment for meeting {}", meeting_id);
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            // MyNotes: search meeting titles, transcripts, the user's notes and summaries.
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?1
             UNION ALL
             SELECT m.id, m.title, n.notes_markdown, n.updated_at
             FROM meetings m
             JOIN meeting_notes n ON m.id = n.meeting_id
             WHERE LOWER(n.notes_markdown) LIKE ?1
             UNION ALL
             SELECT m.id, m.title, s.result, s.updated_at
             FROM meetings m
             JOIN summary_processes s ON m.id = s.meeting_id
             WHERE s.result IS NOT NULL AND LOWER(s.result) LIKE ?1
             UNION ALL
             SELECT m.id, m.title, m.title, m.created_at
             FROM meetings m
             WHERE LOWER(m.title) LIKE ?1",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    /// Works on characters (not bytes) so non-ASCII text can't cause a slicing panic.
    pub(crate) fn get_match_context(transcript: &str, query: &str) -> String {
        let chars: Vec<char> = transcript.chars().collect();
        let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase()).collect();
        let needle: Vec<char> = query.to_lowercase().chars().collect();

        // Only map back when lowercasing preserved char count (true for nearly all text).
        let found = if lower.len() == chars.len() && !needle.is_empty() {
            lower.windows(needle.len()).position(|w| w == needle.as_slice())
        } else {
            None
        };

        match found {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + needle.len() + 100).min(chars.len());
                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.extend(&chars[start_index..end_index]);
                if end_index < chars.len() {
                    context.push_str("...");
                }
                context
            }
            None => chars.iter().take(200).collect(),
        }
    }
}

#[cfg(test)]
mod mynotes_search_tests {
    use super::TranscriptsRepository;

    #[test]
    fn match_context_handles_multibyte_text() {
        let text = format!("{}café décision: ship Friday{}", "é".repeat(150), "ü".repeat(150));
        let ctx = TranscriptsRepository::get_match_context(&text, "DÉCISION");
        assert!(ctx.contains("décision: ship Friday"));
        assert!(ctx.starts_with("...") && ctx.ends_with("..."));
    }

    #[test]
    fn match_context_falls_back_to_prefix() {
        assert_eq!(TranscriptsRepository::get_match_context("hello", "zzz"), "hello");
    }
}
