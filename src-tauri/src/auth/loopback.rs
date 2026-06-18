// 🦀 `use` imports bring items into scope. For traits (BufRead, Write), you must
//    import the trait itself — just having a type that implements it isn't enough;
//    Rust only resolves trait methods when the trait is in scope.
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};

/// A bound loopback listener plus the redirect URI Google should call back.
// 🦀 `struct` defines a named product type. `pub` on a field makes it readable/writable
//    from outside this module; without it the field is private even if the struct is pub.
pub struct Loopback {
    listener: TcpListener,
    pub redirect_uri: String,
}

impl Loopback {
    pub fn bind() -> Result<Self> {
        // 🦀 Port 0 asks the OS to pick a free ephemeral port — useful for tests and OAuth
        //    loopback listeners where you don't need a fixed port.
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AppError::Auth(format!("bind failed: {e}")))?;
        // 🦀 `?` on a Result: if Ok(v), unwrap to v; if Err(e), return early from the
        //    enclosing function with Err(e) (after applying From::from if needed).
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Auth(e.to_string()))?
            .port();
        // 🦀 `Self` inside an impl block is an alias for the type being implemented (Loopback).
        //    Using Self instead of the concrete name means one less thing to update on rename.
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}"),
        })
    }

    /// Block until Google redirects back, then return (code, state).
    // 🦀 `self` by value (not `&self`) *moves* the Loopback into this method, consuming it.
    //    After this call the caller no longer owns the value — the compiler prevents reuse.
    //    Here that's intentional: we only ever want one response from the listener.
    pub fn wait_for_code(self) -> Result<(String, String)> {
        // 🦀 Poll accept() with a deadline so an abandoned sign-in (user closes the
        //    browser, Google never redirects) fails cleanly instead of parking this
        //    thread forever. `set_nonblocking(true)` makes accept() return immediately
        //    with a `WouldBlock` error when no connection is waiting yet.
        self.listener
            .set_nonblocking(true)
            .map_err(|e| AppError::Auth(e.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                // 🦀 `ErrorKind::WouldBlock` is the "nothing ready yet" signal, not a
                //    real failure — sleep briefly and retry until the deadline passes.
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
        // 🦀 An accepted socket can inherit the listener's non-blocking mode; switch it
        //    back to blocking so the `read_line` below simply waits for the request.
        stream
            .set_nonblocking(false)
            .map_err(|e| AppError::Auth(e.to_string()))?;
        let mut request_line = String::new();
        BufReader::new(&stream)
            .read_line(&mut request_line)
            .map_err(|e| AppError::Auth(e.to_string()))?;

        // 🦀 `?` on an Option (inside a function returning Result): ok_or_else converts
        //    None → Err, then `?` returns early with that Err. Handy shorthand.
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

/// Parse the request line `GET /?code=XXX&state=YYY HTTP/1.1` into (code, state).
pub fn parse_redirect_query(request_line: &str) -> Option<(String, String)> {
    // 🦀 `split_whitespace()` returns an iterator over whitespace-delimited tokens.
    //    `.nth(1)` consumes up to the second element and returns Option<&str>.
    //    The trailing `?` is Option's early-return: if None, the function returns None.
    let path = request_line.split_whitespace().nth(1)?; // "/?code=...&state=..."
    // 🦀 `split_once(pat)` splits on the *first* occurrence of pat, returning Option<(&str,&str)>.
    //    `.1` accesses the second element of the tuple (the part after '?').
    let query = path.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    // 🦀 `form_urlencoded::parse` returns an iterator of (Cow<str>, Cow<str>) key-value pairs,
    //    percent-decoding and handling `+`-as-space automatically.
    // 🦀 Closures: `|k, v| { ... }` is an anonymous function (closure). Here we use a
    //    `for` loop over the iterator instead, which is equivalent but more readable.
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            // 🦀 `.into_owned()` on a Cow<str>: if the Cow is Borrowed, it clones to String;
            //    if already Owned, it moves the String out. Either way you get an owned String.
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    // 🦀 `?` on Option inside an Option-returning function: `code?` returns None if code is None,
    //    otherwise unwraps to the inner value. Lets you propagate None without match boilerplate.
    Some((code?, state?))
}

// 🦀 `#[cfg(test)]` is a conditional compilation attribute: this module is compiled only
//    when running `cargo test`, keeping test helpers out of the production binary.
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
