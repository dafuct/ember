// Pure helpers for composing/replying. No I/O — unit-testable once Vitest lands.

// Extract a bare address from a header like "Maya <maya@studio.co>" → "maya@studio.co".
export function parseAddress(headerValue: string): string {
  const m = headerValue.match(/<([^>]+)>/);
  return (m ? m[1] : headerValue).trim();
}

// Split a recipient input on commas/semicolons into trimmed, non-empty addresses.
export function parseRecipients(input: string): string[] {
  return input
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

// Loose validity: one "@", a dot after it, no whitespace.
export function isPlausibleEmail(addr: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(addr);
}

// Prefix "Re: " unless already present (case-insensitive).
export function replySubject(subject: string): string {
  return /^re:/i.test(subject.trim()) ? subject : `Re: ${subject}`;
}

// Build a quoted reply body: a blank gap, an attribution line, then "> "-prefixed original.
export function quoteBody(
  fromLabel: string,
  dateLabel: string,
  text: string,
): string {
  const quoted = text
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
  return `\n\nOn ${dateLabel}, ${fromLabel} wrote:\n${quoted}\n`;
}

// Append a plain-text signature block to a composed body. Empty/whitespace signature
// → body unchanged. The "-- " line is the standard signature delimiter.
export function appendSignature(body: string, signature: string): string {
  const sig = signature.trim();
  if (!sig) return body;
  return `${body}\n\n-- \n${sig}`;
}

// Prefix "Fwd: " unless already present (case-insensitive). Mirrors replySubject.
export function forwardSubject(subject: string): string {
  return /^fwd:/i.test(subject.trim()) ? subject : `Fwd: ${subject}`;
}

// Compute reply-all recipients from the original message.
// To = the original sender (bare address). Cc = the original To + Cc addresses, deduped
// case-insensitively, with YOUR address and the sender removed. Display names are dropped
// (bare addresses only) — a deliberate lean-v1 simplification.
export function replyAllRecipients(
  from: string,
  to: string,
  cc: string,
  self: string,
): { to: string; cc: string } {
  const selfAddr = parseAddress(self).toLowerCase();
  const fromAddr = parseAddress(from).toLowerCase();
  const seen = new Set<string>([selfAddr, fromAddr].filter((a) => a.length > 0));
  const ccOut: string[] = [];
  for (const raw of [...parseRecipients(to), ...parseRecipients(cc)]) {
    const bare = parseAddress(raw);
    const key = bare.toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    ccOut.push(bare);
  }
  return { to: parseAddress(from), cc: ccOut.join(", ") };
}

// The plain-text forwarded-message header block (a blank line trails it, before the body).
export function forwardBlock(
  from: string,
  dateLabel: string,
  subject: string,
  to: string,
): string {
  return [
    "---------- Forwarded message ---------",
    `From: ${from}`,
    `Date: ${dateLabel}`,
    `Subject: ${subject}`,
    `To: ${to}`,
    "",
    "",
  ].join("\n");
}
