// Pure display helpers for attachments. No I/O — safe in the maket.

// Human-readable byte size: 1023 → "1023 B", 2048 → "2.0 KB", 5_242_880 → "5.0 MB".
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let size = n / 1024;
  let i = 0;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(1)} ${units[i]}`;
}

// Last path segment, for displaying a picked file's name (handles / and \).
export function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}
