// src/lib/mock.ts — DEV-ONLY data so the app renders in a plain browser (the "maket").
// Never used in the Tauri build: every call site is guarded by !isTauri().
import type { CalendarEvent } from "./calendar";
import { toYmd } from "./calendar";
import type { MessagePreview, SyncSummary } from "./api";

export const MOCK_ACCOUNT = "you@example.com (mock)";

export const MOCK_MESSAGES: MessagePreview[] = [
  {
    id: "m1", thread_id: "t1", from: "Maya <maya@studio.co>", subject: "Q3 roadmap",
    date: "Wed, 18 Jun 2026 09:42:00 -0700", snippet: "Here's the draft for review…",
    internal_date: 1750000000000, category: "people", label_ids: ["INBOX", "UNREAD"],
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
    default:
      return [];
  }
}
