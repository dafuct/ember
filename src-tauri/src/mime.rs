// 🦀 A pure RFC822 (email) message builder for plain-text mail. No I/O and no clock,
//    so it is fully unit-testable. Gmail fills in Date and Message-ID for us, so this
//    module never touches the system time.

use base64::Engine;

/// A plain-text message to send. `from` is the connected account address; the reply
/// fields are `None` for a fresh compose and `Some(..)` when replying.
pub struct OutgoingMessage {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

// 🦀 Strip CR/LF from a header value so a caller can't inject extra headers — e.g. a
//    crafted Subject, or a sender name carried in from a replied-to email. CR/LF become
//    spaces. The body (after the blank line) is unaffected.
fn sanitize_header(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

// 🦀 NOTE (v1): a long non-ASCII subject yields a single encoded-word that can exceed
//    RFC 2047's 75-char limit. Gmail and modern clients accept it; folding is deferred.
// 🦀 RFC2047 "encoded-word" for a non-ASCII header value: `=?UTF-8?B?<base64>?=`.
//    Pure-ASCII subjects pass through unchanged. `str::is_ascii` is the cheap gate.
fn encode_subject(subject: &str) -> String {
    if subject.is_ascii() {
        subject.to_string()
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(subject.as_bytes());
        format!("=?UTF-8?B?{b64}?=")
    }
}

// 🦀 base64-encode the UTF-8 body and wrap to 76-char lines joined by CRLF (RFC 2045).
//    base64 output is ASCII, so chunking the bytes and re-reading as &str never fails.
fn base64_body(body: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Build the full RFC822 message: headers, a blank line, then the base64 body. Uses
/// CRLF line endings throughout (what SMTP/Gmail expect).
pub fn build_rfc822(msg: &OutgoingMessage) -> String {
    // 🦀 Collect header lines into a Vec, then join with CRLF — clearer than push_str-ing
    //    a String with manual separators.
    let mut headers: Vec<String> = Vec::new();
    headers.push(format!("From: {}", sanitize_header(&msg.from)));
    headers.push(format!("To: {}", msg.to.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    if !msg.cc.is_empty() {
        headers.push(format!("Cc: {}", msg.cc.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    }
    headers.push(format!("Subject: {}", encode_subject(&sanitize_header(&msg.subject))));
    // 🦀 `if let Some(x) = &opt` borrows the inner value without consuming the Option.
    if let Some(irt) = &msg.in_reply_to {
        headers.push(format!("In-Reply-To: {}", sanitize_header(irt)));
    }
    if let Some(refs) = &msg.references {
        headers.push(format!("References: {}", sanitize_header(refs)));
    }
    headers.push("MIME-Version: 1.0".to_string());
    headers.push("Content-Type: text/plain; charset=\"utf-8\"".to_string());
    headers.push("Content-Transfer-Encoding: base64".to_string());
    format!("{}\r\n\r\n{}", headers.join("\r\n"), base64_body(&msg.body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    // 🦀 Decode a wrapped-base64 body back to its String (strip the CRLF wrapping first).
    fn decode_body(s: &str) -> String {
        let joined: String = s.split("\r\n").collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(joined).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    fn msg() -> OutgoingMessage {
        OutgoingMessage {
            from: "me@example.com".into(),
            to: vec!["a@x.com".into()],
            cc: vec![],
            subject: "Hello".into(),
            body: "Hi there".into(),
            in_reply_to: None,
            references: None,
        }
    }

    #[test]
    fn builds_basic_headers_and_body() {
        let out = build_rfc822(&msg());
        assert!(out.contains("From: me@example.com\r\n"));
        assert!(out.contains("To: a@x.com\r\n"));
        assert!(out.contains("Subject: Hello\r\n"));
        assert!(out.contains("MIME-Version: 1.0\r\n"));
        assert!(out.contains("Content-Type: text/plain; charset=\"utf-8\"\r\n"));
        assert!(out.contains("Content-Transfer-Encoding: base64\r\n"));
        let (_, body) = out.split_once("\r\n\r\n").unwrap();
        assert_eq!(decode_body(body), "Hi there");
    }

    #[test]
    fn omits_cc_when_empty_and_joins_when_present() {
        let mut m = msg();
        assert!(!build_rfc822(&m).contains("Cc:"));
        m.cc = vec!["c1@x.com".into(), "c2@x.com".into()];
        assert!(build_rfc822(&m).contains("Cc: c1@x.com, c2@x.com\r\n"));
    }

    #[test]
    fn joins_multiple_to() {
        let mut m = msg();
        m.to = vec!["a@x.com".into(), "b@x.com".into()];
        assert!(build_rfc822(&m).contains("To: a@x.com, b@x.com\r\n"));
    }

    #[test]
    fn encodes_non_ascii_subject_as_rfc2047() {
        let mut m = msg();
        m.subject = "Привіт".into();
        let expected = format!(
            "Subject: =?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode("Привіт".as_bytes())
        );
        assert!(build_rfc822(&m).contains(&expected));
    }

    #[test]
    fn includes_reply_threading_headers() {
        let mut m = msg();
        m.in_reply_to = Some("<abc@mail>".into());
        m.references = Some("<abc@mail>".into());
        let out = build_rfc822(&m);
        assert!(out.contains("In-Reply-To: <abc@mail>\r\n"));
        assert!(out.contains("References: <abc@mail>\r\n"));
    }

    #[test]
    fn body_base64_wraps_at_76_and_roundtrips() {
        let mut m = msg();
        m.body = "x".repeat(200);
        let (_, body) = build_rfc822(&m).split_once("\r\n\r\n").map(|(h, b)| (h.to_string(), b.to_string())).unwrap();
        for line in body.split("\r\n") {
            assert!(line.len() <= 76);
        }
        assert_eq!(decode_body(&body), "x".repeat(200));
    }

    #[test]
    fn no_date_or_message_id_headers() {
        let out = build_rfc822(&msg());
        assert!(!out.contains("Date:"));
        assert!(!out.contains("Message-ID:"));
    }

    #[test]
    fn sanitizes_crlf_in_header_values() {
        let mut m = msg();
        m.subject = "Hi\r\nBcc: evil@x.com".into();
        m.to = vec!["a@x.com\nX-Injected: yes".into()];
        let out = build_rfc822(&m);
        let (headers, _) = out.split_once("\r\n\r\n").unwrap();
        // CR/LF were flattened to spaces — no injected header LINE exists.
        for line in headers.split("\r\n") {
            assert!(!line.starts_with("Bcc:"));
            assert!(!line.starts_with("X-Injected:"));
        }
    }
}
