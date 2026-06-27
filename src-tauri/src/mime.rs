
use base64::Engine;

pub struct OutgoingMessage {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

pub struct OutgoingAttachment {
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

fn sanitize_header(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn encode_subject(subject: &str) -> String {
    if subject.is_ascii() {
        subject.to_string()
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(subject.as_bytes());
        format!("=?UTF-8?B?{b64}?=")
    }
}

pub fn mime_for_ext(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

fn wrap76(encoded: &str) -> String {
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn base64_body(body: &str) -> String {
    wrap76(&base64::engine::general_purpose::STANDARD.encode(body.as_bytes()))
}

fn base64_bytes(bytes: &[u8]) -> String {
    wrap76(&base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn outgoing_headers(msg: &OutgoingMessage) -> Vec<String> {
    let mut headers: Vec<String> = Vec::new();
    headers.push(format!("From: {}", sanitize_header(&msg.from)));
    headers.push(format!("To: {}", msg.to.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    if !msg.cc.is_empty() {
        headers.push(format!("Cc: {}", msg.cc.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    }
    headers.push(format!("Subject: {}", encode_subject(&sanitize_header(&msg.subject))));
    if let Some(irt) = msg.in_reply_to.as_deref().filter(|s| !s.is_empty()) {
        headers.push(format!("In-Reply-To: {}", sanitize_header(irt)));
    }
    if let Some(refs) = msg.references.as_deref().filter(|s| !s.is_empty()) {
        headers.push(format!("References: {}", sanitize_header(refs)));
    }
    headers
}

pub fn build_rfc822(msg: &OutgoingMessage) -> String {
    let mut headers = outgoing_headers(msg);
    headers.push("MIME-Version: 1.0".to_string());
    headers.push("Content-Type: text/plain; charset=\"utf-8\"".to_string());
    headers.push("Content-Transfer-Encoding: base64".to_string());
    format!("{}\r\n\r\n{}", headers.join("\r\n"), base64_body(&msg.body))
}

pub fn build_multipart_rfc822(
    msg: &OutgoingMessage,
    attachments: &[OutgoingAttachment],
    boundary: &str,
) -> String {
    let mut headers = outgoing_headers(msg);
    headers.push("MIME-Version: 1.0".to_string());
    headers.push(format!("Content-Type: multipart/mixed; boundary=\"{boundary}\""));

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "--{boundary}\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
        base64_body(&msg.body)
    ));
    for att in attachments {
        let safe_name = encode_subject(&sanitize_header(&att.filename));
        let mime = sanitize_header(&att.mime_type);
        parts.push(format!(
            "--{boundary}\r\nContent-Type: {mime}; name=\"{safe_name}\"\r\nContent-Disposition: attachment; filename=\"{safe_name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            base64_bytes(&att.bytes)
        ));
    }
    let body = format!("{}\r\n--{boundary}--", parts.join("\r\n"));
    format!("{}\r\n\r\n{}", headers.join("\r\n"), body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

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
    fn omits_empty_threading_headers() {
        let mut m = msg();
        m.in_reply_to = Some(String::new());
        m.references = Some(String::new());
        let out = build_rfc822(&m);
        assert!(!out.contains("In-Reply-To:"));
        assert!(!out.contains("References:"));
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
        for line in headers.split("\r\n") {
            assert!(!line.starts_with("Bcc:"));
            assert!(!line.starts_with("X-Injected:"));
        }
    }

    #[test]
    fn mime_for_ext_maps_known_and_falls_back() {
        assert_eq!(mime_for_ext("report.pdf"), "application/pdf");
        assert_eq!(mime_for_ext("PHOTO.JPG"), "image/jpeg");
        assert_eq!(mime_for_ext("data.unknownext"), "application/octet-stream");
        assert_eq!(mime_for_ext("noextension"), "application/octet-stream");
    }

    #[test]
    fn multipart_has_text_part_and_attachment_roundtrips() {
        let mut m = msg();
        m.body = "see file".into();
        let atts = vec![OutgoingAttachment {
            filename: "a.txt".into(),
            mime_type: "text/plain".into(),
            bytes: b"hello bytes".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "BOUND123");
        assert!(out.contains("Content-Type: multipart/mixed; boundary=\"BOUND123\""));
        assert!(out.contains("--BOUND123\r\n"));
        assert!(out.contains("Content-Disposition: attachment; filename=\"a.txt\""));
        assert!(out.trim_end().ends_with("--BOUND123--"));
        let marker = "Content-Disposition: attachment; filename=\"a.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        let after = out.split(marker).nth(1).unwrap();
        let b64: String = after.split("\r\n--BOUND123--").next().unwrap().split("\r\n").collect();
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert_eq!(decoded, b"hello bytes");
    }

    #[test]
    fn multipart_encodes_non_ascii_filename() {
        let m = msg();
        let atts = vec![OutgoingAttachment {
            filename: "Звіт.pdf".into(),
            mime_type: "application/pdf".into(),
            bytes: b"x".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "B");
        assert!(out.contains("=?UTF-8?B?"));
        assert!(!out.contains("Звіт.pdf"));
    }

    #[test]
    fn multipart_sanitizes_crlf_in_filename() {
        let m = msg();
        let atts = vec![OutgoingAttachment {
            filename: "a\r\nContent-Type: evil".into(),
            mime_type: "text/plain".into(),
            bytes: b"x".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "B");
        for line in out.split("\r\n") {
            assert!(!line.starts_with("Content-Type: evil"));
        }
    }
}
