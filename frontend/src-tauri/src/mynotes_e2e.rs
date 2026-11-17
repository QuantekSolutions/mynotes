//! Headless end-to-end check of the MyNotes pipeline using the real engines:
//! audio file → decode → VAD → Parakeet transcription → saved meeting (SQLite,
//! real migrations) → user notes → local LLM summary (llama-helper sidecar)
//! → Markdown export.
//!
//! Live microphone/system capture and the UI are not exercised here.
//!
//! Run (ignored by default; downloads/uses ~3 GB of local models):
//!   MYNOTES_E2E_WAV=/path/meeting.wav \
//!   MYNOTES_E2E_DATA_DIR="$HOME/Library/Application Support/com.mynotes.desktop" \
//!   MYNOTES_E2E_OUT=/tmp/out \
//!   MEETILY_LLAMA_HELPER=frontend/src-tauri/binaries/llama-helper-aarch64-apple-darwin \
//!   cargo test --release -p meetily --features coreml --lib mynotes_e2e -- --ignored --nocapture

use crate::api::TranscriptSegment;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::mynotes::{format_offset, meeting_markdown_for_test, store_notes};
use crate::parakeet_engine::ParakeetEngine;
use crate::summary::llm_client::LLMProvider;
use crate::summary::processor::generate_meeting_summary;
use crate::summary::templates;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

const USER_NOTES: &str = "- v2.1 ship next Tue (confirmed)\n- ask Daniel: does the crash fix cover tablets?\n- login perf = biggest user complaint";

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn mynotes_e2e_audio_to_markdown() {
    let wav = PathBuf::from(std::env::var("MYNOTES_E2E_WAV").expect("MYNOTES_E2E_WAV"));
    let data_dir = PathBuf::from(std::env::var("MYNOTES_E2E_DATA_DIR").expect("MYNOTES_E2E_DATA_DIR"));
    let out_dir = PathBuf::from(std::env::var("MYNOTES_E2E_OUT").expect("MYNOTES_E2E_OUT"));
    let summary_model = std::env::var("MYNOTES_E2E_SUMMARY_MODEL").unwrap_or_else(|_| "qwen3.5:4b".into());
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Decode + VAD (same functions the import pipeline uses)
    let t = Instant::now();
    let decoded = crate::audio::decoder::decode_audio_file(&wav).expect("decode");
    let samples = decoded.to_whisper_format();
    let segments = crate::audio::vad::get_speech_chunks_with_progress(&samples, 400, |_, _| true).expect("vad");
    println!("[1] decoded {:.1}s audio, {} speech segments ({:?})", decoded.duration_seconds, segments.len(), t.elapsed());
    assert!(!segments.is_empty(), "VAD found no speech");

    // 2. Transcribe with the default local model (Parakeet), downloading it if needed
    let t = Instant::now();
    let engine = ParakeetEngine::new_with_models_dir(Some(data_dir.join("models"))).expect("engine");
    let model = crate::config::DEFAULT_PARAKEET_MODEL;
    let models = engine.discover_models().await.expect("discover");
    let ready = models.iter().any(|m| m.name == model && matches!(m.status, crate::parakeet_engine::ModelStatus::Available));
    if !ready {
        println!("[2] downloading {model}…");
        engine.download_model(model, None).await.expect("download parakeet");
    }
    engine.load_model(model).await.expect("load parakeet");
    let mut transcripts = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let text = engine.transcribe_audio(seg.samples.clone()).await.expect("transcribe");
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let start = seg.start_timestamp_ms / 1000.0;
        let end = seg.end_timestamp_ms / 1000.0;
        println!("    {} {}", format_offset(start), text);
        transcripts.push(TranscriptSegment {
            id: format!("seg-{i}"),
            text,
            timestamp: chrono::Utc::now().to_rfc3339(),
            audio_start_time: Some(start),
            audio_end_time: Some(end),
            duration: Some(end - start),
        });
    }
    println!("[2] transcribed {} segments ({:?})", transcripts.len(), t.elapsed());
    let full_text = transcripts.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
    assert!(full_text.contains("release") || full_text.contains("ship"), "transcript missing expected words: {full_text}");

    // 3. Save meeting + notes into a fresh database with the app's migrations
    let db_path = out_dir.join("e2e.sqlite");
    let _ = std::fs::remove_file(&db_path);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display())).unwrap().create_if_missing(true))
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrations");
    let meeting_id = TranscriptsRepository::save_transcript(&pool, "Weekly product sync", &transcripts, None)
        .await
        .expect("save meeting");
    store_notes(&pool, &meeting_id, USER_NOTES).await.expect("save notes");
    println!("[3] saved meeting {meeting_id} with notes");

    // 4. Summarize with the bundled local LLM, combining notes + transcript
    let t = Instant::now();
    let transcript_text = transcripts
        .iter()
        .map(|s| format!("{} {}", format_offset(s.audio_start_time.unwrap_or(0.0)), s.text))
        .collect::<Vec<_>>()
        .join("\n");
    let template = templates::get_template("standard_meeting").expect("template");
    let notes = crate::mynotes::load_notes(&pool, &meeting_id).await.unwrap().unwrap_or_default();
    let threshold = crate::summary::summary_engine::models::get_model_by_name(&summary_model)
        .map(|m| m.context_size.saturating_sub(300) as usize)
        .unwrap_or(1748);
    let generated = generate_meeting_summary(
        &reqwest::Client::new(),
        &LLMProvider::BuiltInAI,
        &summary_model,
        "",
        &transcript_text,
        "",
        &notes,
        "standard_meeting",
        &template,
        threshold,
        None,
        None,
        None,
        None,
        None,
        Some(&data_dir),
        None,
        Some("en"),
        Some("en"),
        None,
    )
    .await
    .expect("summary generation");
    println!("[4] summary generated with {summary_model} ({:?})", t.elapsed());
    let started = chrono::Utc::now();
    SummaryProcessesRepository::create_or_reset_process(&pool, &meeting_id, started).await.unwrap();
    SummaryProcessesRepository::update_process_completed(
        &pool,
        &meeting_id,
        started,
        serde_json::json!({ "markdown": generated.final_markdown }),
        generated.successful_chunk_count,
        0.0,
    )
    .await
    .unwrap();

    // 5. Search covers transcript, notes and summary
    for q in ["tablets", "release", "weekly product"] {
        let hits = TranscriptsRepository::search_transcripts(&pool, q).await.unwrap();
        println!("[5] search '{q}': {} hit(s)", hits.len());
        assert!(hits.iter().any(|h| h.id == meeting_id), "search for '{q}' did not find the meeting");
    }

    // 6. Export
    let md = meeting_markdown_for_test(&pool, &meeting_id).await.expect("export");
    let export_path = out_dir.join("Weekly product sync.md");
    std::fs::write(&export_path, &md).unwrap();
    println!("[6] exported {} bytes to {}", md.len(), export_path.display());
    for section in ["# Weekly product sync", "## My Notes", "## Summary", "## Transcript", "ask Daniel"] {
        assert!(md.contains(section), "export missing {section}");
    }
    assert!(!md.contains("_No summary has been generated yet._"));
    println!("\n===== EXPORT =====\n{md}");
}
