use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Sanitize untrusted email HTML. Scripts, event handlers, and dangerous URL schemes
/// are always removed (ammonia's allowlist). Inline presentational attributes are kept
/// so emails render. When `load_images` is false, remote `<img>` sources are dropped to
/// block tracking pixels; the returned bool is true if any remote image was blocked.
pub fn sanitize_html(raw: &str, load_images: bool) -> (String, bool) {
    // 🦀 `AtomicBool` behind an `Arc` is a thread-safe shared flag. We clone the Arc into
    //    the filter closure so it can flip the flag, then read it back after cleaning.
    let blocked = Arc::new(AtomicBool::new(false));

    let mut builder = ammonia::Builder::new();
    // 🦀 Allow presentational attributes on every tag so emails keep their styling.
    //    Safe because the frontend renders the result inside a sandboxed iframe.
    builder.generic_attributes(HashSet::from([
        "style", "bgcolor", "width", "height", "align", "valign",
    ]));

    if !load_images {
        // 🦀 `move` transfers ownership of the cloned Arc into the closure so it can
        //    outlive this scope (the builder stores it). The filter runs for every
        //    attribute: returning `None` removes it, `Some(value)` keeps it.
        let flag = blocked.clone();
        builder.attribute_filter(move |element, attribute, value| {
            if element == "img" && (attribute == "src" || attribute == "srcset") {
                let v = value.trim_start();
                if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("//")
                {
                    flag.store(true, Ordering::Relaxed);
                    return None;
                }
            }
            Some(Cow::Borrowed(value))
        });
    }

    let clean = builder.clean(raw).to_string();
    (clean, blocked.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::sanitize_html;

    #[test]
    fn removes_script_and_event_handlers() {
        let (out, _) = sanitize_html("<p onclick=\"evil()\">hi</p><script>alert(1)</script>", true);
        assert!(!out.contains("<script"));
        assert!(!out.contains("onclick"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn removes_javascript_url() {
        let (out, _) = sanitize_html("<a href=\"javascript:alert(1)\">x</a>", true);
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn keeps_inline_style() {
        let (out, _) = sanitize_html("<p style=\"color:red\">x</p>", true);
        assert!(out.contains("style"));
        assert!(out.contains("color"));
    }

    #[test]
    fn blocks_remote_image_when_not_loading() {
        let (out, blocked) = sanitize_html("<img src=\"https://track.example/p.png\">", false);
        assert!(blocked);
        assert!(!out.contains("track.example"));
    }

    #[test]
    fn keeps_remote_image_when_loading() {
        let (out, blocked) = sanitize_html("<img src=\"https://cdn.example/a.png\">", true);
        assert!(!blocked);
        assert!(out.contains("cdn.example"));
    }
}
