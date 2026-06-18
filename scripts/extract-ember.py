#!/usr/bin/env python3
"""Extract the bundled assets and template from the imported Ember Mail design.

`docs/Ember Mail.html` is a self-contained "Design Component" bundle:
  - a <script type="__bundler/manifest"> with base64 (optionally gzip) assets, and
  - a <script type="__bundler/template"> with the JSON-encoded page HTML.

This script regenerates `docs/_ember_extracted/` (gitignored, reproducible) holding
the dc-runtime JS, the template.html markup + logic, the bundled fonts, and the SVG
illustrations. Run:  python3 scripts/extract-ember.py
"""
import base64
import gzip
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SRC = os.path.join(ROOT, "docs", "Ember Mail.html")
OUT = os.path.join(ROOT, "docs", "_ember_extracted")

EXT = {
    "text/javascript": "js", "application/javascript": "js", "text/css": "css",
    "image/png": "png", "image/jpeg": "jpg", "image/svg+xml": "svg",
    "image/webp": "webp", "image/gif": "gif",
    "font/woff2": "woff2", "font/woff": "woff", "font/ttf": "ttf",
}


def grab(lines, tag):
    """Return the payload line that follows the real <script type="__bundler/{tag}"> tag.

    Anchored on the line start so we don't match the same selector string where it
    appears inside the unpacker's own JavaScript.
    """
    for i, line in enumerate(lines):
        if line.strip().startswith(f'<script type="__bundler/{tag}"'):
            return lines[i + 1]
    return None


def main():
    os.makedirs(OUT, exist_ok=True)
    with open(SRC, "r", encoding="utf-8") as f:
        lines = f.readlines()

    summary = []

    manifest = json.loads(grab(lines, "manifest"))
    for uuid, entry in manifest.items():
        data = base64.b64decode(entry["data"])
        if entry.get("compressed"):
            data = gzip.decompress(data)
        mime = entry.get("mime", "application/octet-stream")
        fn = f"{uuid}.{EXT.get(mime, 'bin')}"
        with open(os.path.join(OUT, fn), "wb") as g:
            g.write(data)
        summary.append((len(data), mime, fn))

    html = json.loads(grab(lines, "template"))
    with open(os.path.join(OUT, "template.html"), "w", encoding="utf-8") as g:
        g.write(html)
    summary.append((len(html), "text/html", "template.html"))

    for n, mime, fn in sorted(summary, reverse=True):
        print(f"{n:>10}  {mime:<22}  {fn}")
    print(f"\nExtracted {len(summary)} files to {OUT}")


if __name__ == "__main__":
    main()
