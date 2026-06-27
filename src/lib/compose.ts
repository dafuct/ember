
export function parseAddress(headerValue: string): string {
  const m = headerValue.match(/<([^>]+)>/);
  return (m ? m[1] : headerValue).trim();
}

export function parseRecipients(input: string): string[] {
  return input
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

export function isPlausibleEmail(addr: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(addr);
}

export function replySubject(subject: string): string {
  return /^re:/i.test(subject.trim()) ? subject : `Re: ${subject}`;
}

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

export function appendSignature(body: string, signature: string): string {
  const sig = signature.trim();
  if (!sig) return body;
  return `${body}\n\n-- \n${sig}`;
}

export function forwardSubject(subject: string): string {
  return /^fwd:/i.test(subject.trim()) ? subject : `Fwd: ${subject}`;
}

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
