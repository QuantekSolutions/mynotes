'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Clock, NotebookPen, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import {
  LIVE_NOTES_CHANGED_EVENT,
  readLiveNotesDraft,
  writeLiveNotesDraft,
} from '@/lib/liveNotes';

function formatOffset(totalSeconds: number): string {
  const t = Math.max(0, Math.round(totalSeconds));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `[${h}:${mm}:${ss}]` : `[${mm}:${ss}]`;
}

/**
 * Notes the user types during a meeting (Markdown). Autosaved as a local draft
 * and attached to the meeting when the recording is saved; the summary then
 * combines these notes with the transcript.
 */
export function LiveNotesPanel() {
  const { isRecording, isPaused, recordingDuration } = useRecordingState();
  const [notes, setNotes] = useState('');
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setNotes(readLiveNotesDraft());
    const reload = () => {
      setNotes(readLiveNotesDraft());
      setSavedAt(null);
    };
    window.addEventListener(LIVE_NOTES_CHANGED_EVENT, reload);
    return () => window.removeEventListener(LIVE_NOTES_CHANGED_EVENT, reload);
  }, []);

  // Debounced autosave of the draft
  useEffect(() => {
    const handle = setTimeout(() => {
      if (notes !== readLiveNotesDraft()) {
        writeLiveNotesDraft(notes);
        setSavedAt(new Date());
      }
    }, 400);
    return () => clearTimeout(handle);
  }, [notes]);

  const insertTimestamp = useCallback(() => {
    const el = textareaRef.current;
    const stamp = isRecording && recordingDuration != null
      ? formatOffset(recordingDuration)
      : `[${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}]`;
    const start = el?.selectionStart ?? notes.length;
    const end = el?.selectionEnd ?? notes.length;
    const before = notes.slice(0, start);
    const prefix = before.length === 0 || before.endsWith('\n') ? '' : '\n';
    const insert = `${prefix}- ${stamp} `;
    const next = before + insert + notes.slice(end);
    setNotes(next);
    requestAnimationFrame(() => {
      if (!el) return;
      el.focus();
      const pos = start + insert.length;
      el.setSelectionRange(pos, pos);
    });
  }, [isRecording, recordingDuration, notes]);

  const clearNotes = useCallback(() => {
    if (!notes.trim() || window.confirm('Clear these notes? This cannot be undone.')) {
      setNotes('');
      writeLiveNotesDraft('');
      setSavedAt(null);
    }
  }, [notes]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'j') {
      e.preventDefault();
      insertTimestamp();
    }
  };

  return (
    <aside
      aria-label="My notes"
      className="w-[38%] min-w-[300px] max-w-[520px] shrink-0 bg-white flex flex-col"
      data-testid="live-notes-panel"
    >
      <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200">
        <div className="flex items-center gap-2">
          <NotebookPen className="h-4 w-4 text-gray-600" />
          <h2 className="text-sm font-semibold text-gray-900">My Notes</h2>
          {isRecording && (
            <span className="flex items-center gap-1 text-xs text-red-600" aria-live="polite">
              <span className={`h-2 w-2 rounded-full bg-red-500 ${isPaused ? '' : 'animate-pulse'}`} />
              {isPaused ? 'Paused' : 'Recording'}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="sm" onClick={insertTimestamp} title="Insert timestamp (⌘J)">
            <Clock className="h-4 w-4" />
          </Button>
          <Button variant="ghost" size="sm" onClick={clearNotes} title="Clear notes" disabled={!notes}>
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>
      <textarea
        ref={textareaRef}
        value={notes}
        onChange={(e) => setNotes(e.target.value)}
        onKeyDown={onKeyDown}
        spellCheck
        placeholder={
          'Type notes during the meeting (Markdown works).\n\n' +
          '- Key points you want captured\n- Decisions you heard\n- TODO: follow-ups\n\n' +
          'They are saved with the meeting and combined with the transcript when you generate a summary.'
        }
        className="flex-1 resize-none px-4 py-3 pb-32 text-sm leading-6 text-gray-900 placeholder:text-gray-400 focus:outline-none font-mono"
        data-testid="live-notes-textarea"
      />
      <div className="px-4 py-2 border-t border-gray-100 text-xs text-gray-400">
        {savedAt ? `Draft saved ${savedAt.toLocaleTimeString()}` : 'Notes attach to the meeting when you stop recording'}
      </div>
    </aside>
  );
}
