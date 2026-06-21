// src/lib/mock.ts — DEV-ONLY data so the app renders in a plain browser (the "maket").
// Never used in the Tauri build: every call site is guarded by !isTauri().
import type { CalendarEvent } from "./calendar";
import { toYmd } from "./calendar";
import type { MessagePreview, SyncSummary, DraftContent, Label, MessageBody, Attachment, ReplyContext, EventWrite, CalendarSummary } from "./api";
import type { MeetingNote, MeetingNoteWrite } from "./notes";

export const MOCK_ACCOUNT = "you@example.com (mock)";

export const MOCK_MESSAGES: MessagePreview[] = [
  {
    id: "m1", thread_id: "t1", from: "Maya <maya@studio.co>", subject: "Q3 roadmap",
    date: "Wed, 18 Jun 2026 09:42:00 -0700", snippet: "Here's the draft for review…",
    internal_date: 1750000000000, category: "people", label_ids: ["INBOX", "UNREAD", "Label_1"],
    to_addr: "you@example.com",
  },
  {
    id: "m2", thread_id: "t2", from: "GitHub <noreply@github.com>", subject: "[ember] CI passed",
    date: "Wed, 18 Jun 2026 08:10:00 -0700", snippet: "All checks have passed on m10-calendar.",
    internal_date: 1749990000000, category: "notifications", label_ids: ["INBOX"],
    to_addr: "you@example.com",
  },
];

export const MOCK_SYNC: SyncSummary = { added: 0, removed: 0 };

/** Generate a plausible week of events anchored to the requested window's Monday. */
export function mockCalendarWeek(timeMin: string, _timeMax: string): CalendarEvent[] {
  const mon = new Date(timeMin); // local Monday 00:00 from toTimeMinMax
  const day = (offset: number, h: number, m = 0) => {
    const d = new Date(mon);
    d.setDate(d.getDate() + offset);
    d.setHours(h, m, 0, 0);
    return d.toISOString();
  };
  const ymdAt = (offset: number) => {
    const d = new Date(mon);
    d.setDate(d.getDate() + offset);
    return toYmd(d);
  };
  const ACCENT = "#16a34a";
  const AMBER = "#b9722a";
  return [
    { id: "e1", calendar_id: "primary", title: "Standup", start: day(0, 9), end: day(0, 9, 30), all_day: false, location: null, color: ACCENT },
    { id: "e2", calendar_id: "primary", title: "1:1 with Dana", start: day(0, 14), end: day(0, 15), all_day: false, location: "Zoom", color: ACCENT },
    { id: "e3", calendar_id: "primary", title: "Design review", start: day(1, 11), end: day(1, 12, 30), all_day: false, location: null, color: ACCENT },
    { id: "e4", calendar_id: "personal", title: "Dentist", start: day(2, 8, 30), end: day(2, 9, 30), all_day: false, location: null, color: AMBER },
    { id: "e5", calendar_id: "primary", title: "Team sync", start: day(2, 15), end: day(2, 16), all_day: false, location: null, color: ACCENT },
    { id: "e6", calendar_id: "primary", title: "Roadmap", start: day(3, 10), end: day(3, 11), all_day: false, location: null, color: ACCENT },
    { id: "e7", calendar_id: "personal", title: "Lunch w/ Sam", start: day(3, 10, 30), end: day(3, 11, 30), all_day: false, location: "Cafe", color: AMBER },
    { id: "e8", calendar_id: "primary", title: "Ship M9", start: day(4, 9), end: day(4, 10), all_day: false, location: null, color: ACCENT },
    { id: "e9", calendar_id: "primary", title: "Demo", start: day(4, 16), end: day(4, 17), all_day: false, location: null, color: ACCENT },
    { id: "e10", calendar_id: "personal", title: "Hike", start: day(6, 9, 30), end: day(6, 12), all_day: false, location: null, color: AMBER },
    { id: "e11", calendar_id: "primary", title: "Q3 planning", start: ymdAt(2), end: ymdAt(4), all_day: true, location: null, color: AMBER },
  ];
}

/** Browser-maket search: case-insensitive substring match over the mock messages. */
export function mockSearch(query: string): MessagePreview[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return MOCK_MESSAGES.filter(
    (m) =>
      m.from.toLowerCase().includes(q) ||
      m.subject.toLowerCase().includes(q) ||
      m.snippet.toLowerCase().includes(q),
  );
}

/** Browser-maket folder contents: a small per-folder set so the rail is demoable offline. */
export function mockFolder(folder: string): MessagePreview[] {
  const base = (id: string, from: string, to_addr: string, subject: string, snippet: string): MessagePreview => ({
    id, thread_id: id, from, subject, snippet, to_addr,
    date: "Wed, 18 Jun 2026 09:00:00 -0700", internal_date: 1750000000000, category: "", label_ids: [],
  });
  switch (folder) {
    case "sent":
      return [
        base("s1", "you@example.com", "Maya <maya@studio.co>", "Re: Q3 roadmap", "Sounds good — shipping Friday."),
        base("s2", "you@example.com", "Sam, Dana", "Lunch Thursday?", "Works for me."),
      ];
    case "starred":
      return [base("st1", "Dana <dana@corp.io>", "you@example.com", "Offsite agenda", "Pinned for later.")];
    case "archive":
      return [base("a1", "Newsletter <news@weekly.dev>", "you@example.com", "Weekly digest", "Archived reading.")];
    case "trash":
      return [base("d1", "Spammer <promo@deals.biz>", "you@example.com", "50% OFF!!!", "Trashed.")];
    case "spam":
      return [base("sp1", "Prince <prince@scam.test>", "you@example.com", "Urgent transfer", "Definitely spam.")];
    case "drafts":
      return [
        { ...base("dm1", MOCK_ACCOUNT, "Maya <maya@studio.co>", "Re: Q3 roadmap", "Draft: I think we should…"), draft_id: "dr1" },
        { ...base("dm2", MOCK_ACCOUNT, "", "(no recipient)", "Half-written idea…"), draft_id: "dr2" },
      ];
    default:
      return [];
  }
}

/** Browser-maket: return editable content for a mock draft. */
export function mockGetDraft(draftId: string): DraftContent {
  if (draftId === "dr2") {
    return { draft_id: "dr2", to: "", cc: "", subject: "", body: "Half-written idea…", in_reply_to: null, references: null, thread_id: null };
  }
  return { draft_id: "dr1", to: "Maya <maya@studio.co>", cc: "", subject: "Re: Q3 roadmap", body: "Draft: I think we should…", in_reply_to: null, references: null, thread_id: null };
}

/** Browser-maket: pretend a save succeeded, returning a stable fake draft id. */
export function mockSaveDraft(): string {
  return "dr-mock";
}

export const MOCK_LABELS: Label[] = [
  { id: "Label_1", name: "Work", color: { text: "#ffffff", background: "#16a34a" } },
  { id: "Label_2", name: "Personal" },
];

/** Browser-maket: messages "in" a label = the mock messages carrying that label id. */
export function mockFetchLabel(labelId: string): MessagePreview[] {
  return MOCK_MESSAGES.filter((m) => m.label_ids.includes(labelId));
}

/** Browser-maket: a message body, with attachments on m1 so the strip is demoable. */
export function mockMessageBody(id: string): MessageBody {
  const attachments: Attachment[] =
    id === "m1"
      ? [
          { filename: "Q3-roadmap.pdf", mime_type: "application/pdf", size: 248000, attachment_id: "att1" },
          { filename: "budget.xlsx", mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", size: 18500, attachment_id: "att2" },
        ]
      : [];
  return {
    html: `<p style="font-family:system-ui">(mock body for ${id})</p>`,
    is_html: true,
    blocked_images: false,
    attachments,
  };
}

/** Browser-maket: pretend the user picked a file so the compose chips are demoable. */
export function mockPickFiles(): string[] {
  return ["/Users/you/Documents/proposal.pdf"];
}

/** Browser-maket: echo a created event (fake id, a mock Meet link when requested). */
export function mockCreateEvent(calendarId: string, ev: EventWrite, addMeet: boolean): CalendarEvent {
  return {
    id: `mock-${ev.title.replace(/\s+/g, "-")}`,
    calendar_id: calendarId,
    title: ev.title,
    start: ev.start,
    end: ev.end,
    all_day: ev.all_day,
    location: ev.location,
    color: "#16a34a",
    description: ev.description,
    meet_link: addMeet ? "https://meet.google.com/mock-abc" : null,
    html_link: null,
    attendees: ev.attendees,
  };
}
export function mockUpdateEvent(calendarId: string, eventId: string, ev: EventWrite): CalendarEvent {
  return { ...mockCreateEvent(calendarId, ev, false), id: eventId };
}
export function mockListCalendars(): CalendarSummary[] {
  return [
    { id: "primary", summary: "you@example.com", primary: true, writable: true },
    { id: "personal@group", summary: "Personal", primary: false, writable: true },
  ];
}

/** Browser-maket reply/forward context: gives m1 a Cc (for reply-all) + attachments (for forward). */
export function mockReplyContext(id: string): ReplyContext {
  const attachments: Attachment[] =
    id === "m1"
      ? [
          { filename: "Q3-roadmap.pdf", mime_type: "application/pdf", size: 248000, attachment_id: "att1" },
          { filename: "budget.xlsx", mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", size: 18500, attachment_id: "att2" },
        ]
      : [];
  return {
    message_id: `<${id}@mock>`,
    references: "",
    quoted_text: "Here's the draft for review…",
    to: "you@example.com, Dana <dana@corp.io>",
    cc: id === "m1" ? "Sam <sam@team.io>" : "",
    attachments,
  };
}

// --- Meeting notes (M20) -----------------------------------------------------
// In-memory note store for the browser maket. Keyed by `${calendar_id}|${event_id}`
// inline (NOT importing notes.ts's noteKey, to keep this module's import of notes.ts
// type-only — mirrors how mock.ts imports api.ts). Seeded with two notes on events that
// mockCalendarWeek always produces (e2 "1:1 with Dana", e6 "Roadmap"), so the panel + dots
// are demoable on any visible week.
const mockNoteKey = (calendarId: string, eventId: string) => `${calendarId}|${eventId}`;

const MOCK_NOTES = new Map<string, MeetingNote>([
  [
    mockNoteKey("primary", "e2"),
    {
      id: 1, calendar_id: "primary", event_id: "e2",
      event_title: "1:1 with Dana", event_start: "2026-06-22T14:00:00-07:00",
      body: "- Career growth check-in\n- Reviewed Q3 priorities\n- Action: share the roadmap doc",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_200_000,
      summary: "## Summary\n- Career growth + Q3 priorities discussed\n\n## Action items\n- [ ] Share the roadmap doc",
      summary_updated_at: 1_750_000_100_000,
      transcript: "Dana: How's the quarter going?\nYou: On track — shipping the roadmap doc Friday.",
    },
  ],
  [
    mockNoteKey("primary", "e6"),
    {
      id: 2, calendar_id: "primary", event_id: "e6",
      event_title: "Roadmap", event_start: "2026-06-25T10:00:00-07:00",
      body: "Draft milestones for H2. Decide M21 scope next.",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_100_000,
      summary: "", summary_updated_at: 0,
      transcript: "",
    },
  ],
]);

let mockNoteId = 100; // fresh ids for newly-created mock notes

export function mockGetMeetingNote(calendarId: string, eventId: string): MeetingNote | null {
  return MOCK_NOTES.get(mockNoteKey(calendarId, eventId)) ?? null;
}

export function mockSaveMeetingNote(w: MeetingNoteWrite): MeetingNote {
  const key = mockNoteKey(w.calendar_id, w.event_id);
  const now = 1_750_000_500_000; // fixed clock (no Date.now in maket data, keeps it deterministic)
  const existing = MOCK_NOTES.get(key);
  const note: MeetingNote = {
    id: existing?.id ?? mockNoteId++,
    calendar_id: w.calendar_id,
    event_id: w.event_id,
    event_title: w.event_title,
    event_start: w.event_start,
    body: w.body,
    created_at: existing?.created_at ?? now,
    updated_at: now,
    summary: existing?.summary ?? "",
    summary_updated_at: existing?.summary_updated_at ?? 0,
    transcript: w.transcript,
  };
  MOCK_NOTES.set(key, note);
  return note;
}

export function mockDeleteMeetingNote(calendarId: string, eventId: string): void {
  MOCK_NOTES.delete(mockNoteKey(calendarId, eventId));
}

export function mockListMeetingNotes(): MeetingNote[] {
  return [...MOCK_NOTES.values()].sort((a, b) => b.updated_at - a.updated_at);
}

// Browser-maket: set a canned structured summary on the stored note. summary_updated_at is
// >= the note's updated_at, so the result reads as FRESH (no staleness hint right after).
export function mockSummarizeMeetingNote(calendarId: string, eventId: string): MeetingNote {
  const key = mockNoteKey(calendarId, eventId);
  const existing = MOCK_NOTES.get(key);
  const base: MeetingNote = existing ?? {
    id: mockNoteId++, calendar_id: calendarId, event_id: eventId,
    event_title: "", event_start: "", body: "",
    created_at: 1_750_000_500_000, updated_at: 1_750_000_500_000,
    summary: "", summary_updated_at: 0,
    transcript: "",
  };
  const note: MeetingNote = {
    ...base,
    summary: "## Summary\n- (demo) Key points captured from the notes\n\n## Action items\n- [ ] (demo) Follow up with the team",
    summary_updated_at: Math.max(base.updated_at, 1_750_000_600_000),
  };
  MOCK_NOTES.set(key, note);
  return note;
}

// Browser-maket: pretend a .vtt was picked + parsed to plain text.
export function mockReadTranscriptFile(_path: string): string {
  return "Dana: Welcome everyone.\nYou: Let's review the Q3 priorities.\nDana: Action — share the roadmap doc by Friday.";
}
