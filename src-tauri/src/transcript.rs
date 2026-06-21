// src-tauri/src/transcript.rs — pure transcript helpers (no I/O, fully unit-testable, M22).

/// Convert a WebVTT caption file to plain spoken text: drop the WEBVTT header, NOTE/STYLE/REGION
/// blocks, metadata lines, "-->" timestamp lines, and numeric cue ids; strip inline `<…>` tags;
/// collapse consecutive duplicate lines (rolling captions repeat). Plain `.txt` passes through.
pub fn vtt_to_text(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // 🦀 Skip the WebVTT header + block markers + metadata.
        if t.starts_with("WEBVTT")
            || t.starts_with("NOTE")
            || t.starts_with("STYLE")
            || t.starts_with("REGION")
            || t.starts_with("Kind:")
            || t.starts_with("Language:")
        {
            continue;
        }
        // 🦀 Timestamp cue lines contain the "-->" arrow.
        if t.contains("-->") {
            continue;
        }
        // 🦀 Numeric-only lines are cue identifiers (e.g. "1", "2").
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cleaned = strip_tags(t);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        // 🦀 Collapse a line identical to the previous kept line (rolling captions repeat).
        if out.last().map(|p| p == cleaned).unwrap_or(false) {
            continue;
        }
        out.push(cleaned.to_string());
    }
    out.join("\n")
}

// 🦀 Remove `<…>` segments (e.g. `<v Dana>`, `</v>`, `<00:00:00.000>`). A depth counter handles
//    a stray '>' gracefully and avoids pulling in a regex dependency.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth: u32 = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Build the text fed to the summarizer from the user's notes + the transcript. Both present →
/// labeled sections; only one → that one; neither → "" (the caller guards empty).
pub fn build_summary_input(body: &str, transcript: &str) -> String {
    let b = body.trim();
    let t = transcript.trim();
    match (b.is_empty(), t.is_empty()) {
        (false, false) => format!("Meeting notes:\n{b}\n\nTranscript:\n{t}"),
        (false, true) => b.to_string(),
        (true, false) => t.to_string(),
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_to_text_strips_structure_and_dedupes() {
        let raw = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE recording\n\n\
                   1\n00:00:01.000 --> 00:00:03.000\n<v Dana>Hello everyone</v>\n\n\
                   2\n00:00:03.000 --> 00:00:05.000\nHello everyone\n\n\
                   3\n00:00:05.000 --> 00:00:07.000\nLet's start the review";
        assert_eq!(vtt_to_text(raw), "Hello everyone\nLet's start the review");
    }

    #[test]
    fn vtt_to_text_passes_plain_text_through() {
        let raw = "Just some notes\nwith two lines";
        assert_eq!(vtt_to_text(raw), "Just some notes\nwith two lines");
    }

    #[test]
    fn build_summary_input_combines_or_falls_back() {
        assert_eq!(build_summary_input("notes", "tr"), "Meeting notes:\nnotes\n\nTranscript:\ntr");
        assert_eq!(build_summary_input("notes", "   "), "notes"); // transcript blank → body only
        assert_eq!(build_summary_input("", "tr"), "tr"); // body blank → transcript only
        assert_eq!(build_summary_input("  ", ""), ""); // both blank → empty
    }
}
