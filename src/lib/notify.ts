import { isTauri } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { MessagePreview } from "./api";

export function pickNewMail(
  known: Set<string>,
  list: MessagePreview[],
  cap: number,
): MessagePreview[] {
  const fresh = list.filter((m) => !known.has(m.id));
  fresh.sort((a, b) => b.internal_date - a.internal_date);
  return fresh.slice(0, cap);
}

export function displayName(from: string): string {
  const m = from.match(/^\s*"?([^"<]*?)"?\s*<.*>\s*$/);
  const name = m?.[1]?.trim();
  return name && name.length > 0 ? name : from.trim() || "New mail";
}

export async function ensureNotificationPermission(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  } catch (e) {
    console.warn("[ember] notification permission check failed:", e);
    return false;
  }
}

export async function notifyNewMail(m: MessagePreview, accountLabel?: string): Promise<void> {
  if (!isTauri()) {
    console.debug("[ember] (maket) new mail:", displayName(m.from), "—", m.subject, accountLabel ? `(${accountLabel})` : "");
    return;
  }
  try {
    const title = accountLabel ? `${displayName(m.from)} · ${accountLabel}` : displayName(m.from);
    sendNotification({ title, body: m.subject });
  } catch (e) {
    console.warn("[ember] sendNotification failed:", e);
  }
}
