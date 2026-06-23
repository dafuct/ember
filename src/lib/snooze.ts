import { isTauri, invoke } from "@tauri-apps/api/core";
import { mockSnooze, mockUnsnooze, mockWakeDue, mockListSnoozed } from "./mock";
import type { MessagePreview } from "./api";

export interface SnoozePreset { label: string; wakeAt: number; }
export interface SnoozedRow {
  message_id: string; thread_id: string; wake_at: number; snoozed_at: number;
  from_addr: string; subject: string; snippet: string; internal_date: number;
}

// Preset wake times, anchored to 09:00 local. "This weekend" = the coming Saturday
// (next Saturday if today is already Saturday). "Next week" = next Monday.
export function snoozePresets(now: Date = new Date()): SnoozePreset[] {
  const day = now.getDay(); // 0 Sun .. 6 Sat
  const at9 = (addDays: number): number => {
    const x = new Date(now);
    x.setDate(x.getDate() + addDays);
    x.setHours(9, 0, 0, 0);
    return x.getTime();
  };
  let weekendDays = (6 - day + 7) % 7; // 0 on Sat
  if (day === 6) weekendDays = 7;      // already Saturday → next Saturday
  let mondayDays = (1 - day + 7) % 7;  // 0 on Mon
  if (mondayDays === 0) mondayDays = 7;
  return [
    { label: "Later today", wakeAt: now.getTime() + 3 * 60 * 60 * 1000 },
    { label: "Tomorrow", wakeAt: at9(1) },
    { label: "This weekend", wakeAt: at9(weekendDays) },
    { label: "Next week", wakeAt: at9(mondayDays) },
  ];
}

export const snoozeMessage = (m: MessagePreview, wakeAt: number): Promise<void> =>
  isTauri()
    ? invoke<void>("snooze_message", {
        id: m.id, wakeAt, threadId: m.thread_id, fromAddr: m.from,
        subject: m.subject, snippet: m.snippet, internalDate: m.internal_date,
      })
    : mockSnooze(m, wakeAt);

export const unsnoozeMessage = (id: string): Promise<void> =>
  isTauri() ? invoke<void>("unsnooze_message", { id }) : mockUnsnooze(id);

export const wakeDueSnoozes = (): Promise<string[]> =>
  isTauri() ? invoke<string[]>("wake_due_snoozes") : mockWakeDue();

export const listSnoozed = (): Promise<SnoozedRow[]> =>
  isTauri() ? invoke<SnoozedRow[]>("list_snoozed") : mockListSnoozed();
