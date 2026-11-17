import { afterAll, beforeEach, expect, mock, test } from 'bun:test';

const calls: Array<[string, Record<string, unknown> | undefined]> = [];
let failSave = false;
const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
  removeItem: (k: string) => void store.delete(k),
};

const live = await import('../../src/lib/liveNotes');
live.__setInvokeForTests((async (command: string, args?: Record<string, unknown>) => {
  calls.push([command, args]);
  if (command === 'mynotes_save_notes' && failSave) throw new Error('db locked');
  return null;
}) as any);
afterAll(() => live.__setInvokeForTests(null));

beforeEach(() => {
  calls.length = 0;
  store.clear();
  failSave = false;
});

test('attaches the draft to the saved meeting and clears it', async () => {
  live.writeLiveNotesDraft('- decision: ship Friday');
  expect(await live.attachLiveNotesToMeeting('m1')).toBe(true);
  expect(calls).toEqual([['mynotes_save_notes', { meetingId: 'm1', notesMarkdown: '- decision: ship Friday' }]]);
  expect(live.readLiveNotesDraft()).toBe('');
});

test('does nothing when the draft is blank', async () => {
  live.writeLiveNotesDraft('   \n ');
  expect(await live.attachLiveNotesToMeeting('m1')).toBe(false);
  expect(calls).toEqual([]);
});

test('keeps the draft when saving fails', async () => {
  failSave = true;
  live.writeLiveNotesDraft('important');
  await expect(live.attachLiveNotesToMeeting('m1')).rejects.toThrow('db locked');
  expect(live.readLiveNotesDraft()).toBe('important');
});

test('export passes reveal and null path by default', async () => {
  await live.exportMeetingMarkdown('m2', { reveal: true });
  expect(calls).toEqual([['mynotes_export_markdown', { meetingId: 'm2', path: null, reveal: true }]]);
});
