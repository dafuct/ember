use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};

pub struct Loopback {
    listener: TcpListener,
    pub redirect_uri: String,
}

impl Loopback {
    pub fn bind() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AppError::Auth(format!("bind failed: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Auth(e.to_string()))?
            .port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}"),
        })
    }

    pub fn wait_for_code(self) -> Result<(String, String)> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| AppError::Auth(e.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(AppError::Auth(
                            "timed out waiting for the Google sign-in redirect".into(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(AppError::Auth(e.to_string())),
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|e| AppError::Auth(e.to_string()))?;
        let mut request_line = String::new();
        BufReader::new(&stream)
            .read_line(&mut request_line)
            .map_err(|e| AppError::Auth(e.to_string()))?;

        // Google redirects back with `?error=...` when the user is blocked (e.g. the
        // account isn't a Test user of an unverified app) or denies consent. Surface a
        // clear, actionable message instead of a cryptic "missing code" / timeout.
        if let Some(err) = parse_redirect_error(&request_line) {
            write_html(
                &mut stream,
                "<h2>Sign-in didn't finish</h2><p>You can close this tab and return to Ember.</p>",
            );
            return Err(AppError::Auth(format!(
                "Google sign-in was blocked or denied ({err}). If this account isn't a Test user \
                 of the app's Google Cloud project, add it as a Test user — or use your own \
                 Google credentials in Settings → Google API."
            )));
        }

        let (code, state) = parse_redirect_query(&request_line)
            .ok_or_else(|| AppError::Auth("missing code/state in redirect".into()))?;

        write_html(
            &mut stream,
            "<h2>Ember is connected</h2><p>You can close this tab and return to the app.</p>",
        );
        Ok((code, state))
    }
}

fn write_html(stream: &mut TcpStream, inner: &str) {
    let body = format!(
        "<html><body style=\"font-family:sans-serif;text-align:center;padding:40px\">{inner}</body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).ok();
}

pub fn parse_redirect_query(request_line: &str) -> Option<(String, String)> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

/// Extract the OAuth `error` param (e.g. `access_denied`) from the redirect request
/// line, if present. Returned when Google refuses the sign-in rather than handing back
/// an authorization code.
pub fn parse_redirect_error(request_line: &str) -> Option<String> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        if k == "error" {
            return Some(v.into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_redirect_error, parse_redirect_query};

    #[test]
    fn parses_code_and_state_with_percent_encoding() {
        let line = "GET /?code=4%2F0Ab&state=xyz789 HTTP/1.1";
        assert_eq!(
            parse_redirect_query(line),
            Some(("4/0Ab".to_string(), "xyz789".to_string()))
        );
    }

    #[test]
    fn returns_none_without_code() {
        let line = "GET /?state=only HTTP/1.1";
        assert_eq!(parse_redirect_query(line), None);
    }

    #[test]
    fn parses_error_param() {
        let line = "GET /?error=access_denied&state=xyz789 HTTP/1.1";
        assert_eq!(
            parse_redirect_error(line),
            Some("access_denied".to_string())
        );
    }

    #[test]
    fn no_error_when_code_present() {
        let line = "GET /?code=4%2F0Ab&state=xyz789 HTTP/1.1";
        assert_eq!(parse_redirect_error(line), None);
    }
}
