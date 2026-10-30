//! Small HTTP helpers: header checks, cookies, query strings and responses.

use std::io::Write;

use serde_json::Value;
use tiny_http::{Header, Request, Response};

/// Sent with every response (MVP-043). Styles may be inline (the component library sets CSS
/// variables that way); scripts come only from this server.
pub const SECURITY_HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
    ),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
];

pub fn header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// Whether `Host` names this server: `127.0.0.1:<port>` or `localhost:<port>`. Anything else
/// may be a DNS-rebinding page (FR-288).
pub fn host_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// Whether `Origin` is this server's own. `required` for a `POST`; a same-origin `GET` may
/// carry none.
pub fn origin_allowed(origin: Option<&str>, port: u16, required: bool) -> bool {
    match origin {
        None => !required,
        Some(o) => {
            let o = o.trim().to_ascii_lowercase();
            o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
        }
    }
}

/// The value of cookie `name`.
pub fn cookie(req: &Request, name: &str) -> Option<String> {
    let all = header(req, "Cookie")?;
    cookie_value(&all, name)
}

pub fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// Compare secrets in time that does not depend on where they differ.
pub fn same_secret(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Decode `%XX` escapes and `+`.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The decoded value of `name` in a query string.
pub fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        (percent_decode(k) == name).then(|| percent_decode(v))
    })
}

/// Whether `next` is a path on this server: it starts with one `/`, so it cannot name
/// another site (`//evil`, `https://…`).
pub fn local_next(next: &str) -> bool {
    next.starts_with('/')
        && !next.starts_with("//")
        && !next.contains('\\')
        && !next.contains("://")
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn with_security<R: std::io::Read>(mut resp: Response<R>) -> Response<R> {
    for (k, v) in SECURITY_HEADERS {
        if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp.add_header(h);
        }
    }
    resp
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("a valid header")
}

pub fn respond_bytes(req: Request, status: u16, ctype: &str, cache: &str, body: &[u8]) {
    let resp = Response::from_data(body.to_vec())
        .with_status_code(status)
        .with_header(h("Content-Type", ctype))
        .with_header(h("Cache-Control", cache));
    let _ = req.respond(with_security(resp));
}

pub fn respond_text(req: Request, status: u16, text: &str) {
    respond_bytes(
        req,
        status,
        "text/plain; charset=utf-8",
        "no-store",
        text.as_bytes(),
    );
}

pub fn respond_html(req: Request, status: u16, html: &str) {
    respond_bytes(
        req,
        status,
        "text/html; charset=utf-8",
        "no-store",
        html.as_bytes(),
    );
}

pub fn respond_json(req: Request, status: u16, v: &Value) {
    let body = serde_json::to_vec(v).unwrap_or_default();
    respond_bytes(req, status, "application/json", "no-store", &body);
}

/// `303 See Other` to `location`, setting `cookie`.
pub fn redirect(req: Request, location: &str, cookie: &str) {
    let resp = Response::empty(303)
        .with_header(h("Location", location))
        .with_header(h("Set-Cookie", cookie))
        .with_header(h("Cache-Control", "no-store"));
    let _ = req.respond(with_security(resp));
}

/// Server-Sent Events over a raw connection.
pub struct EventStream<'a> {
    out: &'a mut Box<dyn Write + Send>,
}

impl<'a> EventStream<'a> {
    pub fn new(out: &'a mut Box<dyn Write + Send>) -> EventStream<'a> {
        EventStream { out }
    }

    pub fn raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.out.write_all(bytes)?;
        self.out.flush()
    }

    /// One message: `id: <event>.<log>` (when given), `event: <name>`, `data: <json>`.
    pub fn send(
        &mut self,
        id: Option<(u64, u64)>,
        event: &str,
        data: &Value,
    ) -> std::io::Result<()> {
        let mut msg = String::new();
        if let Some((e, l)) = id {
            msg.push_str(&format!("id: {e}.{l}\n"));
        }
        msg.push_str(&format!("event: {event}\n"));
        msg.push_str(&format!(
            "data: {}\n\n",
            serde_json::to_string(data).unwrap_or_default()
        ));
        self.raw(msg.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_hosts_and_own_origins() {
        assert!(host_allowed(Some("127.0.0.1:4100"), 4100));
        assert!(host_allowed(Some("localhost:4100"), 4100));
        assert!(!host_allowed(Some("127.0.0.1:4101"), 4100));
        assert!(!host_allowed(Some("evil.example:4100"), 4100));
        assert!(!host_allowed(None, 4100));
        assert!(origin_allowed(Some("http://localhost:4100"), 4100, true));
        assert!(!origin_allowed(None, 4100, true));
        assert!(origin_allowed(None, 4100, false));
        assert!(!origin_allowed(Some("http://evil.example"), 4100, false));
        assert!(!origin_allowed(Some("null"), 4100, false));
    }

    #[test]
    fn cookies_queries_and_redirects() {
        assert_eq!(
            cookie_value("a=1; resonance_abc=tok; b=2", "resonance_abc").as_deref(),
            Some("tok")
        );
        assert_eq!(cookie_value("resonance_abcd=x", "resonance_abc"), None);
        assert_eq!(
            query_param("token=ab&next=%2Fspec%2FFR-001", "next").as_deref(),
            Some("/spec/FR-001")
        );
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a+b%20c"), "a b c");
        assert!(local_next("/spec/FR-001"));
        assert!(!local_next("//evil.example"));
        assert!(!local_next("https://evil.example"));
        assert!(!local_next("/\\evil.example"));
        assert!(same_secret(b"abc", b"abc"));
        assert!(!same_secret(b"abc", b"abd"));
        assert!(!same_secret(b"", b""));
    }
}
