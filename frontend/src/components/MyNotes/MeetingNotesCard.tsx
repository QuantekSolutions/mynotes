'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, Copy, Download, NotebookPen } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { exportMeetingMarkdown, getMeetingMarkdown, getMeetingNotes, saveMeetingNotes } from '@/lib/liveNotes';

interface MeetingNotesCardProps {
  meetingId: string;
  /** Persist any unsaved summary edits so exports include them. */
  flushPendingEdits?: () => Promise<void>;
}

type SaveState = 'idle' | 'saving' | 'saved' | 'error';

/**
 * Shows and edits the notes the user typed for a meeting, and exports the
 * meeting (notes + summary + transcript) as Markdown.
 */
export function MeetingNotesCard({ meetingId, flushPendingEdits }: MeetingNotesCardProps) {
  const [notes, setNotes] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [expanded, setExpanded] = useState(true);
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [exporting, setExporting] = useState(false);
  const lastSaved = useRef('');

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    getMeetingNotes(meetingId)
      .then((value) => {
        if (cancelled) return;
        setNotes(value);
        lastSaved.current = value;
        setExpanded(true);
      })
      .catch((error) => console.error('Failed to load notes:', error))
      .finally(() => !cancelled && setLoaded(true));
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const persist = useCallback(async (value: string) => {
    if (value === lastSaved.current) return;
    setSaveState('saving');
    try {
      await saveMeetingNotes(meetingId, value);
      lastSaved.current = value;
      setSaveState('saved');
    } catch (error) {
      console.error('Failed to save notes:', error);
      setSaveState('error');
    }
  }, [meetingId]);

  // Debounced autosave
  useEffect(() => {
    if (!loaded) return;
    const handle = setTimeout(() => void persist(notes), 600);
    return () => clearTimeout(handle);
  }, [notes, loaded, persist]);

  const flushAll = useCallback(async () => {
    await persist(notes);
    await flushPendingEdits?.();
  }, [notes, persist, flushPendingEdits]);

  const handleExport = useCallback(async () => {
    setExporting(true);
    try {
      await flushAll();
      const path = await exportMeetingMarkdown(meetingId, { reveal: true });
      toast.success('Exported to Markdown', { description: path });
    } catch (error) {
      toast.error('Export failed', { description: String(error) });
    } finally {
      setExporting(false);
    }
  }, [flushAll, meetingId]);

  const handleCopy = useCallback(async () => {
    try {
      await flushAll();
      const md = await getMeetingMarkdown(meetingId);
      await navigator.clipboard.writeText(md);
      toast.success('Meeting copied as Markdown');
    } catch (error) {
      toast.error('Copy failed', { description: String(error) });
    }
  }, [flushAll, meetingId]);

  const status =
    saveState === 'saving' ? 'Saving…' :
    saveState === 'saved' ? 'Saved' :
    saveState === 'error' ? 'Not saved — retrying on next edit' : '';

  return (
    <section className="shrink-0 border-b border-gray-200 bg-white" data-testid="meeting-notes-card">
      <div className="flex items-center justify-between px-4 py-2">
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="flex items-center gap-2 text-sm font-semibold text-gray-900"
          aria-expanded={expanded}
        >
          {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          <NotebookPen className="h-4 w-4 text-gray-600" />
          My Notes
          <span className="ml-2 text-xs font-normal text-gray-400">{status}</span>
        </button>
        <div className="flex items-center gap-1">
          <Button variant="outline" size="sm" onClick={handleCopy} title="Copy notes, summary and transcript as Markdown">
            <Copy className="h-4 w-4" />
            <span className="hidden lg:inline">Copy Markdown</span>
          </Button>
          <Button variant="outline" size="sm" onClick={handleExport} disabled={exporting} title="Export to ~/Documents/MyNotes">
            <Download className="h-4 w-4" />
            <span className="hidden lg:inline">{exporting ? 'Exporting…' : 'Export .md'}</span>
          </Button>
        </div>
      </div>
      {expanded && (
        <div className="px-4 pb-3">
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            onBlur={() => void persist(notes)}
            disabled={!loaded}
            rows={Math.min(12, Math.max(4, notes.split('\n').length + 1))}
            placeholder="No notes for this meeting yet. Anything you add here is used the next time you generate the summary."
            className="w-full resize-y rounded-md border border-gray-200 bg-gray-50 px-3 py-2 font-mono text-sm leading-6 text-gray-900 placeholder:text-gray-400 focus:border-blue-400 focus:bg-white focus:outline-none"
            data-testid="meeting-notes-textarea"
          />
          <p className="mt-1 text-xs text-gray-400">
            Your notes are combined with the transcript when you generate or regenerate the summary.
          </p>
        </div>
      )}
    </section>
  );
}
