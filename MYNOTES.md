# MyNotes

MyNotes is a private, local meeting notes app for macOS. It is a customized build of
[Meetily](https://github.com/Zackriya-Solutions/meetily) v0.5.0 by Zackriya Solutions
(MIT License — see [LICENSE.md](LICENSE.md); the upstream copyright notice is preserved there
and in the app's About screen).

## What it does

- Records **microphone + system audio** with visible Start / Pause / Stop controls.
- Shows **live transcription** (local Parakeet TDT 0.6B v3 by default; Whisper optional).
- **My Notes pane** next to the live transcript: type Markdown notes during the meeting
  (⌘J inserts a timestamp). The draft is autosaved locally and attached to the meeting when
  you stop recording.
- **Summaries combine your notes with the transcript**: summary, key decisions, action items,
  open questions, discussion highlights. Summaries are editable.
- **No invented facts**: owners only when explicitly named ("Unassigned" otherwise), due dates
  only when explicitly stated ("Not specified" otherwise), decisions only when agreed.
- **Searchable history**: the sidebar search covers meeting titles, transcripts, your notes and
  summaries.
- **Markdown export**: on a meeting, *Export .md* writes `~/Documents/MyNotes/<date> <title>.md`
  (notes + summary + timestamped transcript) and reveals it in Finder; *Copy Markdown* copies it.

## Launch

Built app: `target/release/bundle/macos/MyNotes.app` (also a `.dmg` next to it under
`bundle/dmg/`). Drag `MyNotes.app` into `/Applications` and open it.

The build is ad-hoc signed (not notarized). If macOS blocks the first launch, right-click the
app → **Open**, or run `xattr -dr com.apple.quarantine /Applications/MyNotes.app`.

On first launch, grant **Microphone** (for your voice) and **Screen & System Audio Recording**
(for other participants' audio). Onboarding downloads the local models if they are not already
present (Parakeet ≈ 0.7 GB, Qwen 3.5 4B ≈ 2.6 GB) into
`~/Library/Application Support/com.mynotes.desktop/models/`.

## Costs

Everything runs locally by default — no API keys and no per-minute fees. Cloud summary
providers (OpenAI, Anthropic, Groq, OpenRouter, custom OpenAI-compatible) remain available in
Settings but are **off by default**; enabling one sends transcripts to that provider and bills
your account with them. Anonymous usage analytics are off unless you opt in. Auto-update is
disabled in this fork (it would otherwise install stock Meetily).

## Develop

Prereqs (macOS, Apple Silicon): Xcode CLT, Homebrew `cmake`, Node 20, Rust 1.100 (pinned by
`rust-toolchain.toml`), pnpm 9.15.9.

```bash
cargo build -p llama-helper --release --features metal
mkdir -p frontend/src-tauri/binaries
cp target/release/llama-helper frontend/src-tauri/binaries/llama-helper-aarch64-apple-darwin
cd frontend
npx pnpm@9.15.9 install --frozen-lockfile
npx pnpm@9.15.9 run tauri:dev     # or tauri:build
```

Checks:

```bash
cd frontend && npx tsc --noEmit -p . && npx bun@1.3.14 test
cargo test --release -p meetily --features coreml --lib
```

Headless pipeline test (audio file → transcription → saved meeting → notes → local summary →
search → export), using the real local models; see `frontend/src-tauri/src/mynotes_e2e.rs`.

## MyNotes changes vs. Meetily v0.5.0

- `src-tauri/src/mynotes.rs`: notes persistence (`meeting_notes` table), Markdown export, commands.
- Summary pipeline: user notes passed as `<user_notes>`; stricter no-invention rules; notes are
  part of the summary cache key; reworked *Standard Meeting* template.
- Search extended to notes, summaries and titles; fixed a UTF-8 slicing panic in search snippets.
- UI: live notes pane, meeting notes card with export/copy, MyNotes branding, attribution in About.
- Config: identifier `com.mynotes.desktop`, updater feed removed, Rust toolchain pinned to 1.100
  (Rust 1.97 produced proc-macro dylibs that macOS 27's loader rejects).
