//! The daemon's wire (docs/architecture.md, decision 8): a minimal
//! HTTP/1.1, its WebSocket upgrade (RFC 6455) and the JSON its frames
//! carry. Plain blocking reads and writes on any stream; no TLS, no
//! authentication, which nginx and the socket's permissions give.

pub mod json;

use hbds_core::sha1;

use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};

/// A request: its method, path, headers (names in lowercase) and body.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// The largest body a request may carry, and the largest message a
/// WebSocket may, whole or in fragments: a program's A-code fits.
const MAX_BODY: usize = 64 << 20;

/// The largest head: the first line and the headers together.
const MAX_HEAD: usize = 64 << 10;

/// The largest control frame (RFC 6455, 5.5).
const MAX_CONTROL: u64 = 125;

fn bad(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// The next line of a head, counted against what is `left` of it: a
/// line that never ends is refused, not read for good.
fn head_line(r: &mut impl BufRead, left: &mut usize) -> io::Result<String> {
    let mut line = String::new();
    let n = r.by_ref().take(*left as u64).read_line(&mut line)?;
    if n == *left && !line.ends_with('\n') {
        return Err(bad(format!("a head of more than {MAX_HEAD} bytes")));
    }
    *left -= n;
    Ok(line)
}

/// The headers, names in lowercase, up to the empty line.
fn headers(r: &mut impl BufRead, left: &mut usize) -> io::Result<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    loop {
        let h = head_line(r, left)?;
        if h.is_empty() {
            return Err(bad("the headers end early"));
        }
        let h = h.trim_end_matches(['\r', '\n']);
        if h.is_empty() {
            return Ok(headers);
        }
        let (name, value) = h
            .split_once(':')
            .ok_or_else(|| bad(format!("a header: {h:?}")))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
}

impl Request {
    /// The next request on `r`, or `None` once the peer has closed.
    pub fn read(r: &mut impl BufRead) -> io::Result<Option<Request>> {
        let mut left = MAX_HEAD;
        let line = head_line(r, &mut left)?;
        if line.is_empty() {
            return Ok(None);
        }
        let mut parts = line.split_whitespace();
        let (Some(method), Some(path), Some(version)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(bad(format!("not a request line: {line:?}")));
        };
        if !version.starts_with("HTTP/1.") {
            return Err(bad(format!("not HTTP/1: {version}")));
        }
        let headers = headers(r, &mut left)?;
        let length: usize = match headers.get("content-length") {
            Some(n) => n.parse().map_err(|_| bad(format!("a length: {n:?}")))?,
            None => 0,
        };
        if length > MAX_BODY {
            return Err(bad(format!("a body of {length} bytes")));
        }
        let mut body = vec![0; length];
        r.read_exact(&mut body)?;
        Ok(Some(Request {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body,
        }))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// Writes it, as a client does.
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let mut head = format!("{} {} HTTP/1.1\r\n", self.method, self.path);
        for (name, value) in &self.headers {
            head += &format!("{name}: {value}\r\n");
        }
        if !self.body.is_empty() {
            head += &format!("content-length: {}\r\n", self.body.len());
        }
        head += "\r\n";
        w.write_all(head.as_bytes())?;
        w.write_all(&self.body)?;
        w.flush()
    }
}

/// A response: its status, reason, headers and body.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, reason: &str, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status,
            reason: reason.to_string(),
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    pub fn text(status: u16, reason: &str, body: &str) -> Response {
        let mut r = Response::new(status, reason, body);
        r.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        r
    }

    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, self.reason);
        for (name, value) in &self.headers {
            head += &format!("{name}: {value}\r\n");
        }
        if self.status != 101 {
            head += &format!("content-length: {}\r\n", self.body.len());
        }
        head += "\r\n";
        w.write_all(head.as_bytes())?;
        w.write_all(&self.body)?;
        w.flush()
    }

    /// The response to a request, as a client reads it.
    pub fn read(r: &mut impl BufRead) -> io::Result<Response> {
        let mut left = MAX_HEAD;
        let line = head_line(r, &mut left)?;
        let mut parts = line.trim_end().splitn(3, ' ');
        let (Some(_), Some(status)) = (parts.next(), parts.next()) else {
            return Err(bad(format!("not a status line: {line:?}")));
        };
        let status = status
            .parse()
            .map_err(|_| bad(format!("a status: {status:?}")))?;
        let reason = parts.next().unwrap_or("").to_string();
        let headers = headers(r, &mut left)?;
        let length: usize = match headers.get("content-length") {
            Some(n) => n.parse().map_err(|_| bad(format!("a length: {n:?}")))?,
            None => 0,
        };
        let mut body = vec![0; length.min(MAX_BODY)];
        r.read_exact(&mut body)?;
        Ok(Response {
            status,
            reason,
            headers,
            body,
        })
    }
}

/// Base64, RFC 4648, with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What a server answers to a WebSocket key (RFC 6455, 4.2.2).
pub fn accept_key(key: &str) -> String {
    let mut input = key.trim().as_bytes().to_vec();
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&sha1::digest(&input))
}

/// Whether `request` asks for a WebSocket.
pub fn is_upgrade(request: &Request) -> bool {
    request
        .header("upgrade")
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"))
        && request.header("sec-websocket-key").is_some()
}

/// The server's answer to a WebSocket upgrade.
pub fn upgrade(request: &Request) -> Response {
    let key = request.header("sec-websocket-key").unwrap_or_default();
    let mut r = Response::new(101, "Switching Protocols", Vec::new());
    r.headers.insert("upgrade".into(), "websocket".into());
    r.headers.insert("connection".into(), "Upgrade".into());
    r.headers
        .insert("sec-websocket-accept".into(), accept_key(key));
    r
}

/// A client's WebSocket upgrade request for `path`.
pub fn upgrade_request(path: &str) -> Request {
    let mut headers = BTreeMap::new();
    headers.insert("host".into(), "localhost".into());
    headers.insert("upgrade".into(), "websocket".into());
    headers.insert("connection".into(), "Upgrade".into());
    headers.insert("sec-websocket-version".into(), "13".into());
    // Any 16 bytes do: the socket is local, the key only proves the
    // server speaks WebSocket.
    headers.insert("sec-websocket-key".into(), base64(b"embryo-systems!!"));
    Request {
        method: "GET".into(),
        path: path.into(),
        headers,
        body: Vec::new(),
    }
}

/// A WebSocket message, or the end of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// The fragments of a message read so far: control frames may come
/// between them (RFC 6455, 5.4), and are given as they come.
#[derive(Debug, Default)]
pub struct Partial {
    kind: Option<u8>,
    payload: Vec<u8>,
}

/// Reads the next message; fragments are put back together, those read
/// before a control frame kept in `partial` for the call that follows.
pub fn read_message(r: &mut impl Read, partial: &mut Partial) -> io::Result<Message> {
    loop {
        let mut head = [0u8; 2];
        r.read_exact(&mut head)?;
        let fin = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0f;
        let masked = head[1] & 0x80 != 0;
        let mut length = (head[1] & 0x7f) as u64;
        if length == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            length = u16::from_be_bytes(b) as u64;
        } else if length == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            length = u64::from_be_bytes(b);
        }
        let control = opcode >= 0x8;
        if control && (!fin || length > MAX_CONTROL) {
            return Err(bad("a control frame in fragments, or over 125 bytes"));
        }
        if !control && partial.payload.len() as u64 + length > MAX_BODY as u64 {
            return Err(bad(format!("a message of more than {MAX_BODY} bytes")));
        }
        let mut mask = [0u8; 4];
        if masked {
            r.read_exact(&mut mask)?;
        }
        let mut data = vec![0u8; length as usize];
        r.read_exact(&mut data)?;
        if masked {
            for (i, b) in data.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match opcode {
            0x8 => return Ok(Message::Close),
            0x9 => return Ok(Message::Ping(data)),
            0xa => return Ok(Message::Pong(data)),
            0x0 if partial.kind.is_none() => return Err(bad("a fragment that continues nothing")),
            0x0 => {}
            _ if partial.kind.is_some() => {
                return Err(bad("a message begins before the last has ended"));
            }
            op => partial.kind = Some(op),
        }
        partial.payload.extend(data);
        if fin {
            break;
        }
    }
    let payload = std::mem::take(&mut partial.payload);
    match partial.kind.take() {
        Some(0x1) => String::from_utf8(payload)
            .map(Message::Text)
            .map_err(|_| bad("a text frame that is not UTF-8")),
        Some(0x2) => Ok(Message::Binary(payload)),
        k => Err(bad(format!("a frame of opcode {k:?}"))),
    }
}

/// Writes `message` in one frame; a client masks it (RFC 6455, 5.3).
pub fn write_message(w: &mut impl Write, message: &Message, mask: bool) -> io::Result<()> {
    let (opcode, data): (u8, &[u8]) = match message {
        Message::Text(t) => (0x1, t.as_bytes()),
        Message::Binary(b) => (0x2, b),
        Message::Ping(b) => (0x9, b),
        Message::Pong(b) => (0xa, b),
        Message::Close => (0x8, &[]),
    };
    let mut frame = vec![0x80 | opcode];
    let bit = if mask { 0x80 } else { 0 };
    match data.len() {
        n if n < 126 => frame.push(bit | n as u8),
        n if n <= u16::MAX as usize => {
            frame.push(bit | 126);
            frame.extend((n as u16).to_be_bytes());
        }
        n => {
            frame.push(bit | 127);
            frame.extend((n as u64).to_be_bytes());
        }
    }
    if mask {
        // The mask guards proxies' caches, which a local socket lacks.
        let key = [0x37, 0xfa, 0x21, 0x3d];
        frame.extend(key);
        frame.extend(data.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
    } else {
        frame.extend_from_slice(data);
    }
    w.write_all(&frame)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn base64_as_rfc_4648() {
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), output);
        }
    }

    #[test]
    fn the_accept_key_of_rfc_6455() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn a_request_reads_back() {
        let text =
            "POST /x HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\n\r\nhelloGET / HTTP/1.1\r\n\r\n";
        let mut r = BufReader::new(Cursor::new(text));
        let first = Request::read(&mut r).unwrap().unwrap();
        assert_eq!((first.method.as_str(), first.path.as_str()), ("POST", "/x"));
        assert_eq!(first.header("host"), Some("h"));
        assert_eq!(first.body, b"hello");
        let second = Request::read(&mut r).unwrap().unwrap();
        assert_eq!(second.path, "/");
        assert_eq!(Request::read(&mut r).unwrap(), None);
    }

    #[test]
    fn a_response_reads_back() {
        let mut out = Vec::new();
        Response::text(404, "Not Found", "nothing here")
            .write(&mut out)
            .unwrap();
        let r = Response::read(&mut BufReader::new(Cursor::new(out))).unwrap();
        assert_eq!((r.status, r.reason.as_str()), (404, "Not Found"));
        assert_eq!(r.body, b"nothing here");
    }

    #[test]
    fn the_masked_hello_of_rfc_6455() {
        // Section 5.7: a single-frame masked text message.
        let frame = [
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        let m = read_message(&mut Cursor::new(frame), &mut Partial::default()).unwrap();
        assert_eq!(m, Message::Text("Hello".into()));
        let mut out = Vec::new();
        write_message(&mut out, &m, true).unwrap();
        assert_eq!(out, frame);
    }

    #[test]
    fn fragments_and_long_frames() {
        // Section 5.7: "Hel" then "lo", unmasked.
        let frames = [0x01, 0x03, 0x48, 0x65, 0x6c, 0x80, 0x02, 0x6c, 0x6f];
        let m = read_message(&mut Cursor::new(frames), &mut Partial::default()).unwrap();
        assert_eq!(m, Message::Text("Hello".into()));
        for n in [125, 126, 65535, 65536] {
            let text = "x".repeat(n);
            let mut out = Vec::new();
            write_message(&mut out, &Message::Text(text.clone()), n % 2 == 0).unwrap();
            assert_eq!(
                read_message(&mut Cursor::new(out), &mut Partial::default()).unwrap(),
                Message::Text(text)
            );
        }
    }

    #[test]
    fn a_ping_between_fragments_loses_none() {
        // "Hel", a ping, "lo": the ping is given first, the text whole.
        let frames = [
            0x01, 0x03, 0x48, 0x65, 0x6c, 0x89, 0x01, 0x21, 0x80, 0x02, 0x6c, 0x6f,
        ];
        let (mut r, mut partial) = (Cursor::new(frames), Partial::default());
        let first = read_message(&mut r, &mut partial).unwrap();
        assert_eq!(first, Message::Ping(vec![0x21]));
        let second = read_message(&mut r, &mut partial).unwrap();
        assert_eq!(second, Message::Text("Hello".into()));
    }

    #[test]
    fn frames_out_of_order_are_refused() {
        for frames in [
            // A continuation of nothing.
            &[0x80, 0x01, 0x48][..],
            // A text beginning inside another.
            &[0x01, 0x01, 0x48, 0x81, 0x01, 0x48],
            // A ping in fragments, and one of 126 bytes.
            &[0x09, 0x00],
            &[0x89, 0x7e, 0x00, 0x7e],
        ] {
            let read = read_message(&mut Cursor::new(frames), &mut Partial::default());
            assert!(read.is_err(), "{frames:?}");
        }
    }

    #[test]
    fn a_message_in_fragments_has_a_size() {
        // Fragments of 32 MiB, never the last: the third is one too many.
        let piece = vec![0u8; 32 << 20];
        let mut frames = Vec::new();
        for i in 0..3 {
            frames.push(if i == 0 { 0x02 } else { 0x00 });
            frames.push(127);
            frames.extend((piece.len() as u64).to_be_bytes());
            frames.extend(&piece);
        }
        let e = read_message(&mut Cursor::new(frames), &mut Partial::default()).unwrap_err();
        assert!(e.to_string().contains("a message of more than"), "{e}");
    }

    #[test]
    fn a_head_has_a_size() {
        let endless = "GET /".to_string() + &"x".repeat(1 << 20);
        let e = Request::read(&mut BufReader::new(Cursor::new(endless))).unwrap_err();
        assert!(e.to_string().contains("a head of more than"), "{e}");
        let many = "GET / HTTP/1.1\r\n".to_string() + &"a: b\r\n".repeat(20_000);
        let e = Request::read(&mut BufReader::new(Cursor::new(many.clone()))).unwrap_err();
        assert!(e.to_string().contains("a head of more than"), "{e}");
        let status = many.replace("GET / HTTP/1.1", "HTTP/1.1 200 OK");
        assert!(Response::read(&mut BufReader::new(Cursor::new(status))).is_err());
        // Just under: the limit counts bytes, not lines.
        let fits = "GET / HTTP/1.1\r\n".to_string() + &"a: b\r\n".repeat(10_000) + "\r\n";
        assert!(Request::read(&mut BufReader::new(Cursor::new(fits))).is_ok());
    }
}
