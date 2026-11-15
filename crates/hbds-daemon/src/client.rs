//! The CLI as a client (docs/architecture.md, decision 8): a program's
//! A-code sent to a daemon as a session, its output written and its input
//! read here.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use hbds_http::json::Json;
use hbds_http::{Message, Request, Response};

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
    let mut partial = hbds_http::Partial::default();
    loop {
        let text = match hbds_http::read_message(&mut reader, &mut partial)? {
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

/// `embryo-systems ui`: a browser reaches no Unix socket, so this listens
/// on 127.0.0.1 only, and forwards each connection to the daemon's
/// socket, as the user running it. The daemon keeps its one door
/// (docs/architecture.md, decision 8). Prints the page's address.
pub fn forward(socket: &Path, port: u16) -> io::Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    // Is the daemon there? Better said now than at the first request.
    UnixStream::connect(socket)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", socket.display())))?;
    println!("http://{}/", listener.local_addr()?);
    io::stdout().flush()?;
    let port = listener.local_addr()?.port();
    for stream in listener.incoming() {
        let Ok(browser) = stream else { continue };
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            let _ = forward_one(browser, &socket, port);
        });
    }
    Ok(())
}

/// Whether a request is the page's own: sent to this address, and from
/// no other origin. A page of another site reaches 127.0.0.1 through
/// the user's browser, under a name of its own that resolves here, and
/// a WebSocket crosses origins freely: either would run code against
/// the store as the user.
fn own(request: &Request, port: u16) -> bool {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host = request
        .header("host")
        .is_some_and(|h| hosts.contains(&h.to_string()));
    let origin = match request.header("origin") {
        Some(o) => hosts.iter().any(|h| o == format!("http://{h}")),
        None => true,
    };
    host && origin
}

/// One browser's connection: its request checked, then forwarded, and
/// all that follows copied both ways.
fn forward_one(browser: std::net::TcpStream, socket: &Path, port: u16) -> io::Result<()> {
    browser.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut from_browser = BufReader::new(browser.try_clone()?);
    let mut to_browser = browser;
    let Some(mut request) = Request::read(&mut from_browser)? else {
        return Ok(());
    };
    if !own(&request, port) {
        let no = "not this page's request: its Host or its Origin is another's\n";
        return Response::text(403, "Forbidden", no).write(&mut to_browser);
    }
    to_browser.set_read_timeout(None)?;
    let daemon = UnixStream::connect(socket)?;
    let mut to_daemon = daemon.try_clone()?;
    // Written again with its body's length.
    request.headers.remove("content-length");
    request.write(&mut to_daemon)?;
    let up = std::thread::spawn(move || {
        // What the reader holds beyond the request goes first.
        let _ = io::copy(&mut from_browser, &mut to_daemon);
        let _ = to_daemon.shutdown(std::net::Shutdown::Write);
    });
    let mut from_daemon = daemon;
    let _ = io::copy(&mut from_daemon, &mut to_browser);
    let _ = to_browser.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
    Ok(())
}
