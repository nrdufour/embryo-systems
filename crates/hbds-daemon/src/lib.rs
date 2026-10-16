//! The daemon (docs/architecture.md, decisions 3 and 8): one executor
//! thread owns the store and runs the clients' sessions in turns, a slice
//! of instructions each, ending only between statements; a thread per
//! connection talks to it by channel. One door, a Unix socket: a
//! WebSocket on `/session` runs a program, JSON frames each way; `GET
//! /store` gives the store's dump, `GET /sessions` how many run, `GET
//! /journal` how far the journal has got. On a durable store, what a
//! client hears waits until what came before it is on disk (decision 5).
//!
//! Client to daemon: `{"run": ACODE}`, `{"input": LINE}`, `{"eof": true}`.
//! Daemon to client: `{"output": TEXT}`, `{"input?": true}`, and last
//! `{"end": null}` or `{"end": ERROR}`.

pub mod client;

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

use hbds_acode::Unit;
use hbds_gtad::Store;
use hbds_http::json::Json;
use hbds_http::{Message, Request, Response};
use hbds_journal::Durable;
use hbds_vm::{Language, Session, Turn};

/// The executor's stack: the VM's, for code the store calls back.
const STACK: usize = 256 << 20;

/// How long an idle executor waits before it looks at the journal again.
const IDLE: std::time::Duration = std::time::Duration::from_millis(100);

/// What a connection tells the executor.
enum Msg {
    Start {
        id: u64,
        acode: String,
        events: Events,
        input: Receiver<Option<String>>,
    },
    /// A line of input, or its end, is on the session's channel.
    Wake(u64),
    Gone(u64),
    Dump(Sender<String>),
    Sessions(Sender<usize>),
}

/// What the executor tells a session's connection, and how many of the
/// journal's transactions must be on disk before the client hears it.
struct Event {
    what: What,
    after: u64,
}

enum What {
    Output(String),
    WantsInput,
    End(Option<String>),
}

impl What {
    fn json(&self) -> Json {
        match self {
            What::Output(t) => Json::one("output", Json::String(t.clone())),
            What::WantsInput => Json::one("input?", Json::Bool(true)),
            What::End(None) => Json::one("end", Json::Null),
            What::End(Some(e)) => Json::one("end", Json::String(e.clone())),
        }
    }
}

/// Where events go, stamped with the journal's transactions so far.
#[derive(Clone)]
struct Events {
    to: Sender<Event>,
    durable: Option<Durable>,
}

impl Events {
    fn send(&self, what: What) {
        let after = self.durable.as_ref().map_or(0, |d| d.stats().submitted);
        let _ = self.to.send(Event { what, after });
    }
}

/// A session's input and output: its client's, by channel.
struct Wire {
    /// Lines come on `from`, `None` for the end of the input.
    from: Receiver<Option<String>>,
    lines: VecDeque<String>,
    ended: bool,
    events: Events,
}

impl Wire {
    fn take(&mut self, line: Option<String>) {
        match line {
            Some(l) => self.lines.push_back(l),
            None => self.ended = true,
        }
    }
}

impl hbds_vm::Io for Wire {
    /// A READ in code the store calls back cannot wait for another turn:
    /// the executor waits on this client.
    fn read_line(&mut self) -> Option<String> {
        if !self.ready() {
            self.events.send(What::WantsInput);
            match self.from.recv() {
                Ok(line) => self.take(line),
                Err(_) => self.ended = true,
            }
        }
        self.lines.pop_front()
    }

    fn write(&mut self, text: &str) {
        self.events.send(What::Output(text.to_string()));
    }

    fn ready(&mut self) -> bool {
        while let Ok(line) = self.from.try_recv() {
            self.take(line);
        }
        self.ended || !self.lines.is_empty()
    }
}

struct Client {
    session: Session,
    wire: Wire,
    /// Waiting for a line it asked for.
    waiting: bool,
}

/// Serves `store` on `socket` until the process ends. `slice` is a turn's
/// instructions.
/// How the daemon runs: a turn's instructions, and the journal's lines
/// past which it compacts.
#[derive(Clone, Copy)]
pub struct Pace {
    pub slice: u64,
    pub compact_at: u64,
}

pub fn serve(
    store: Store,
    durable: Option<Durable>,
    socket: &Path,
    language: &'static (dyn Language + Sync),
    pace: Pace,
) -> io::Result<()> {
    if socket.exists() {
        // Left by a daemon that died: the store's lock says none is
        // running on this store, but another may be on this socket.
        if UnixStream::connect(socket).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{}: a daemon already listens here", socket.display()),
            ));
        }
        std::fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    let (tx, rx) = mpsc::channel();
    let journal = durable.clone();
    std::thread::Builder::new()
        .name("executor".into())
        .stack_size(STACK)
        .spawn(move || executor(store, journal, rx, language, pace))?;
    let mut next = 0;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        next += 1;
        let (tx, id, durable) = (tx.clone(), next, durable.clone());
        std::thread::spawn(move || {
            let _ = connection(stream, id, tx, durable);
        });
    }
    Ok(())
}

/// One client's connection: a request, or a session's WebSocket.
fn connection(
    stream: UnixStream,
    id: u64,
    tx: Sender<Msg>,
    durable: Option<Durable>,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let Some(request) = Request::read(&mut reader)? else {
        return Ok(());
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/store") => {
            let (reply, dump) = mpsc::channel();
            let _ = tx.send(Msg::Dump(reply));
            let text = dump.recv().unwrap_or_default();
            Response::text(200, "OK", &text).write(&mut writer)
        }
        ("GET", "/sessions") => {
            let (reply, count) = mpsc::channel();
            let _ = tx.send(Msg::Sessions(reply));
            let n = count.recv().unwrap_or_default();
            Response::text(200, "OK", &format!("{n}\n")).write(&mut writer)
        }
        ("GET", "/journal") => {
            let s = durable.as_ref().map(Durable::stats).unwrap_or_default();
            let n = |x: u64| Json::Number(x as f64);
            let j = Json::Object(BTreeMap::from([
                ("durable".to_string(), Json::Bool(durable.is_some())),
                ("submitted".to_string(), n(s.submitted)),
                ("synced".to_string(), n(s.synced)),
                ("syncs".to_string(), n(s.syncs)),
                ("compactions".to_string(), n(s.compactions)),
                ("lines".to_string(), n(s.lines)),
            ]));
            Response::text(200, "OK", &format!("{j}\n")).write(&mut writer)
        }
        ("GET", "/session") if hbds_http::is_upgrade(&request) => {
            hbds_http::upgrade(&request).write(&mut writer)?;
            session(reader, writer, id, tx, durable)
        }
        _ => Response::text(404, "Not Found", "no such thing here\n").write(&mut writer),
    }
}

fn session(
    mut reader: BufReader<UnixStream>,
    writer: UnixStream,
    id: u64,
    tx: Sender<Msg>,
    durable: Option<Durable>,
) -> io::Result<()> {
    let (to, from_executor) = mpsc::channel();
    let events = Events {
        to,
        durable: durable.clone(),
    };
    let (input, lines) = mpsc::channel();
    let mut lines = Some(lines);
    // The executor's events, written as they come, on a thread of their
    // own: the client's frames are read here meanwhile.
    let mut out = writer.try_clone()?;
    let write = std::thread::spawn(move || {
        for e in from_executor {
            // Done after the sync: the client hears nothing that the
            // journal could still lose what came before.
            if let Some(d) = &durable
                && let Err(failure) = d.wait(e.after)
            {
                let end = What::End(Some(failure)).json().to_string();
                let _ = hbds_http::write_message(&mut out, &Message::Text(end), false);
                let _ = hbds_http::write_message(&mut out, &Message::Close, false);
                return;
            }
            let end = matches!(e.what, What::End(_));
            let text = e.what.json().to_string();
            if hbds_http::write_message(&mut out, &Message::Text(text), false).is_err() {
                return;
            }
            if end {
                let _ = hbds_http::write_message(&mut out, &Message::Close, false);
                return;
            }
        }
        // The executor is gone without a word: the client hears it.
        let gone = What::End(Some("the daemon stopped running this session".into()));
        let _ = hbds_http::write_message(&mut out, &Message::Text(gone.json().to_string()), false);
        let _ = hbds_http::write_message(&mut out, &Message::Close, false);
        let _ = out.shutdown(std::net::Shutdown::Both);
    });
    let mut events = Some(events);
    loop {
        let message = match hbds_http::read_message(&mut reader) {
            Ok(m) => m,
            Err(_) => Message::Close,
        };
        let frame = match message {
            Message::Text(t) => Json::parse(&t).ok(),
            Message::Ping(p) => {
                let mut w = writer.try_clone()?;
                hbds_http::write_message(&mut w, &Message::Pong(p), false)?;
                continue;
            }
            Message::Close => break,
            _ => None,
        };
        let Some(frame) = frame else { continue };
        if let Some(acode) = frame.get("run").and_then(Json::as_str)
            && let (Some(events), Some(input)) = (events.take(), lines.take())
        {
            let acode = acode.to_string();
            if tx
                .send(Msg::Start {
                    id,
                    acode,
                    events,
                    input,
                })
                .is_err()
            {
                break;
            }
        } else if let Some(line) = frame.get("input").and_then(Json::as_str) {
            let _ = input.send(Some(line.to_string()));
            let _ = tx.send(Msg::Wake(id));
        } else if frame.get("eof").is_some() {
            let _ = input.send(None);
            let _ = tx.send(Msg::Wake(id));
        }
    }
    let _ = tx.send(Msg::Gone(id));
    let _ = write.join();
    Ok(())
}

/// The executor: owns the store, runs each ready session for a slice in
/// turn, round robin; waits for messages when none is ready.
fn executor(
    mut store: Store,
    durable: Option<Durable>,
    rx: Receiver<Msg>,
    language: &'static (dyn Language + Sync),
    pace: Pace,
) {
    let mut clients: BTreeMap<u64, Client> = BTreeMap::new();
    let mut after = 0;
    loop {
        let ready = clients.values().any(|c| !c.waiting);
        let first = match ready {
            true => rx.try_recv().ok(),
            // Idle, the journal may still be catching up: it is looked at
            // now and then, to compact what the last turns left.
            false => match rx.recv_timeout(IDLE) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(d) = &durable {
                        d.maybe_compact(&store, pace.compact_at);
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
        };
        for msg in first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
        {
            match msg {
                Msg::Start {
                    id,
                    acode,
                    events,
                    input,
                } => match start(&acode, language) {
                    Ok(session) => {
                        let wire = Wire {
                            from: input,
                            lines: VecDeque::new(),
                            ended: false,
                            events,
                        };
                        clients.insert(
                            id,
                            Client {
                                session,
                                wire,
                                waiting: false,
                            },
                        );
                    }
                    Err(e) => events.send(What::End(Some(e))),
                },
                Msg::Wake(id) => {
                    if let Some(c) = clients.get_mut(&id) {
                        c.waiting = false;
                    }
                }
                Msg::Gone(id) => {
                    clients.remove(&id);
                }
                Msg::Dump(reply) => {
                    let _ = reply.send(store.dump());
                }
                Msg::Sessions(reply) => {
                    let _ = reply.send(clients.len());
                }
            }
        }
        // The next ready client after the last one served.
        let next = (clients.range(after + 1..).chain(clients.range(..=after)))
            .find(|(_, c)| !c.waiting)
            .map(|(&id, _)| id);
        let Some(id) = next else { continue };
        after = id;
        let c = clients.get_mut(&id).expect("a client");
        match c
            .session
            .turn(&mut store, &mut c.wire, language, Some(pace.slice))
        {
            Turn::Yield => {}
            Turn::Input => {
                c.waiting = true;
                c.wire.events.send(What::WantsInput);
            }
            Turn::Done(_) => {
                c.wire.events.send(What::End(None));
                clients.remove(&id);
            }
            Turn::Failed(e) => {
                c.wire.events.send(What::End(Some(e.to_string())));
                clients.remove(&id);
            }
        }
        if let Some(d) = &durable {
            d.maybe_compact(&store, pace.compact_at);
        }
    }
}

/// A session for the A-code in `text`, if this daemon can run it.
fn start(text: &str, language: &dyn Language) -> Result<Session, String> {
    let unit = Unit::read(text).map_err(|e| e.0)?;
    if unit.language != language.name() {
        return Err(format!(
            "A-code in {}, a language this daemon does not have",
            unit.language
        ));
    }
    Ok(Session::new(unit, None))
}
