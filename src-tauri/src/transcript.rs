
pub fn vtt_to_text(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("WEBVTT")
            || t.starts_with("NOTE")
            || t.starts_with("STYLE")
            || t.starts_with("REGION")
            || t.starts_with("Kind:")
            || t.starts_with("Language:")
        {
            continue;
        }
        if t.contains("-->") {
            continue;
        }
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cleaned = strip_tags(t);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        if out.last().map(|p| p == cleaned).unwrap_or(false) {
            continue;
        }
        out.push(cleaned.to_string());
    }
    out.join("\n")
}

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
        assert_eq!(build_summary_input("notes", "   "), "notes");
        assert_eq!(build_summary_input("", "tr"), "tr");
        assert_eq!(build_summary_input("  ", ""), "");
    }
}
