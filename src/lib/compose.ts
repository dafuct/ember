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
