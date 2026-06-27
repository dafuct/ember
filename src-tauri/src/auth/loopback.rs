use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
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

        let (code, state) = parse_redirect_query(&request_line)
            .ok_or_else(|| AppError::Auth("missing code/state in redirect".into()))?;

        let body = "<html><body style=\"font-family:sans-serif;text-align:center;padding:40px\">\
            <h2>Ember is connected</h2><p>You can close this tab and return to the app.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).ok();
        Ok((code, state))
    }
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

#[cfg(test)]
mod tests {
    use super::parse_redirect_query;

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
}
