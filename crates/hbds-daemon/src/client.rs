//! The CLI as a client (docs/architecture.md, decision 8): a program's
//! A-code sent to a daemon as a session, its output written and its input
//! read here.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use hbds_http::json::Json;
use hbds_http::{Message, Response};

fn broken(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.into())
}

/// Runs `acode` on the daemon at `socket`: `Ok(None)` if it ended well,
/// `Ok(Some(error))` if it failed there.
pub fn run(
    socket: &Path,
    acode: &str,
    output: &mut dyn Write,
    input: &mut dyn BufRead,
) -> io::Result<Option<String>> {
    let stream = UnixStream::connect(socket)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", socket.display())))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    hbds_http::upgrade_request("/session").write(&mut writer)?;
    let response = Response::read(&mut reader)?;
    if response.status != 101 {
        return Err(broken(format!("the daemon answers {}", response.status)));
    }
    let send = |w: &mut UnixStream, j: Json| {
        hbds_http::write_message(w, &Message::Text(j.to_string()), true)
    };
    send(
        &mut writer,
        Json::one("run", Json::String(acode.to_string())),
    )?;
    loop {
        let text = match hbds_http::read_message(&mut reader)? {
            Message::Text(t) => t,
            Message::Close => return Err(broken("the daemon closed the session")),
            _ => continue,
        };
        let frame = Json::parse(&text).map_err(broken)?;
        if let Some(t) = frame.get("output").and_then(Json::as_str) {
            output.write_all(t.as_bytes())?;
            output.flush()?;
        } else if frame.get("input?").is_some() {
            let mut line = String::new();
            match input.read_line(&mut line)? {
                0 => send(&mut writer, Json::one("eof", Json::Bool(true)))?,
                _ => {
                    let line = line.trim_end_matches(['\n', '\r']).to_string();
                    send(&mut writer, Json::one("input", Json::String(line)))?
                }
            }
        } else if let Some(end) = frame.get("end") {
            let _ = hbds_http::write_message(&mut writer, &Message::Close, true);
            return Ok(end.as_str().map(String::from));
        }
    }
}
