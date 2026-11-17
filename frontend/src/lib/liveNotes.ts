/**
 * MyNotes: draft storage for notes typed during a live meeting.
 *
 * The draft lives in localStorage so it survives a crash or reload mid-meeting.
 * When the meeting is saved, the draft is written to the meeting_notes table
 * (via `mynotes_save_notes`) and cleared here.
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core';

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
let invoke: Invoke = tauriInvoke;

/** Test seam: replace the Tauri invoker. */
export function __setInvokeForTests(fn: Invoke | null): void {
  invoke = fn ?? tauriInvoke;
}

const DRAFT_KEY = 'mynotes.liveNotesDraft';
export const LIVE_NOTES_CHANGED_EVENT = 'mynotes:live-notes-changed';

export function readLiveNotesDraft(): string {
  try {
    return localStorage.getItem(DRAFT_KEY) ?? '';
  } catch {
    return '';
  }
}

export function writeLiveNotesDraft(value: string): void {
  try {
    if (value) localStorage.setItem(DRAFT_KEY, value);
    else localStorage.removeItem(DRAFT_KEY);
  } catch {
    // Storage unavailable: the in-memory textarea value is still used on save.
  }
}

export function clearLiveNotesDraft(): void {
  writeLiveNotesDraft('');
  try {
    window.dispatchEvent(new Event(LIVE_NOTES_CHANGED_EVENT));
  } catch {
    // no-op outside the browser
  }
}

export async function getMeetingNotes(meetingId: string): Promise<string> {
  return (await invoke<string | null>('mynotes_get_notes', { meetingId })) ?? '';
}

export async function saveMeetingNotes(meetingId: string, notesMarkdown: string): Promise<void> {
  await invoke('mynotes_save_notes', { meetingId, notesMarkdown });
}

/**
 * Attaches the current live draft to a freshly saved meeting.
 * Returns true if notes were attached. The draft is only cleared on success.
 */
export async function attachLiveNotesToMeeting(meetingId: string): Promise<boolean> {
  const draft = readLiveNotesDraft();
  if (!draft.trim()) return false;
  await saveMeetingNotes(meetingId, draft);
  clearLiveNotesDraft();
  return true;
}

export async function exportMeetingMarkdown(
  meetingId: string,
  options: { path?: string; reveal?: boolean } = {},
): Promise<string> {
  return invoke<string>('mynotes_export_markdown', {
    meetingId,
    path: options.path ?? null,
    reveal: options.reveal ?? false,
  });
}

export async function getMeetingMarkdown(meetingId: string): Promise<string> {
  return invoke<string>('mynotes_meeting_markdown', { meetingId });
}
