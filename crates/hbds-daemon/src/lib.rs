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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

use hbds_acode::Unit;
use hbds_gtad::Store;
use hbds_http::json::Json;
use hbds_http::{Message, Request, Response};
use hbds_journal::Durable;
use std::time::{Duration, Instant};

use hbds_gtad::AdtRef;
use hbds_vm::{Agent, Bound, Language, Sequencing, Session, Turn};

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
    /// The pace set, if given; what it is now, answered.
    Pace(Option<Clock>, Sender<String>),
    Time(Sender<String>),
}

/// How the store's time moves (docs/architecture.md, decision 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clock {
    Paused,
    /// This many agents due, then paused.
    Step(u64),
    /// As fast as possible, up to a horizon.
    Fast(f64),
    /// Real time, times a factor.
    Real(f64),
}

impl Clock {
    /// From `{"pace": ..., "horizon": T, "factor": F, "steps": N}`.
    pub fn from_json(j: &Json) -> Result<Clock, String> {
        let number = |key: &str, default: f64| match j.get(key) {
            None => Ok(default),
            Some(Json::Number(n)) => Ok(*n),
            Some(v) => Err(format!("{key} is a number, not {v}")),
        };
        match j.get("pace").and_then(Json::as_str) {
            Some("paused") => Ok(Clock::Paused),
            Some("step") => Ok(Clock::Step(number("steps", 1.0)?.max(1.0) as u64)),
            Some("fast") => Ok(Clock::Fast(number("horizon", f64::INFINITY)?)),
            Some("real") => match number("factor", 1.0)? {
                f if f > 0.0 => Ok(Clock::Real(f)),
                f => Err(format!("a factor of {f}: it is more than 0")),
            },
            _ => Err("a pace is paused, step, fast or real".into()),
        }
    }

    fn json(&self) -> Json {
        let one = |k: &str| Json::String(k.into());
        let mut o = BTreeMap::new();
        match *self {
            Clock::Paused => {
                o.insert("pace".into(), one("paused"));
            }
            Clock::Step(n) => {
                o.insert("pace".into(), one("step"));
                o.insert("steps".into(), Json::Number(n as f64));
            }
            Clock::Fast(h) => {
                o.insert("pace".into(), one("fast"));
                if h.is_finite() {
                    o.insert("horizon".into(), Json::Number(h));
                }
            }
            Clock::Real(f) => {
                o.insert("pace".into(), one("real"));
                o.insert("factor".into(), Json::Number(f));
            }
        }
        Json::Object(o)
    }
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
        let after = self.durable.as_ref().map_or(0, Durable::stamp);
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
    /// Holding: waiting for the kernel to reach its time.
    held: bool,
}

/// What the kernel's processes print: to the client that activated the
/// one running, while it is connected; to the daemon's output otherwise,
/// until the UI has its feed (6d). They read nothing.
struct Router {
    clients: BTreeMap<u64, Events>,
    owner: Option<u64>,
}

impl hbds_vm::Io for Router {
    fn read_line(&mut self) -> Option<String> {
        None
    }

    fn write(&mut self, text: &str) {
        match self.owner.and_then(|id| self.clients.get(&id)) {
            Some(events) => events.send(What::Output(text.to_string())),
            None => print!("{text}"),
        }
    }

    fn running(&mut self, owner: Option<Agent>) {
        self.owner = match owner {
            Some(Agent::Client(id)) => Some(id),
            _ => None,
        };
    }
}

/// Serves `store` on `socket` until the process ends. `slice` is a turn's
/// instructions.
/// How the daemon runs: a turn's instructions, the journal's lines past
/// which it compacts, and the pace its time starts at.
#[derive(Clone, Copy)]
pub struct Pace {
    pub slice: u64,
    pub compact_at: u64,
    pub clock: Clock,
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
        ("GET" | "POST", "/pace") => {
            let clock = match request.method.as_str() {
                "GET" => None,
                _ => {
                    let body = String::from_utf8_lossy(&request.body).to_string();
                    match Json::parse(&body).and_then(|j| Clock::from_json(&j)) {
                        Ok(c) => Some(c),
                        Err(e) => {
                            return Response::text(400, "Bad Request", &format!("{e}\n"))
                                .write(&mut writer);
                        }
                    }
                }
            };
            let (reply, answer) = mpsc::channel();
            let _ = tx.send(Msg::Pace(clock, reply));
            let text = answer.recv().unwrap_or_default();
            Response::text(200, "OK", &format!("{text}\n")).write(&mut writer)
        }
        ("GET", "/time") => {
            let (reply, answer) = mpsc::channel();
            let _ = tx.send(Msg::Time(reply));
            let text = answer.recv().unwrap_or_default();
            Response::text(200, "OK", &format!("{text}\n")).write(&mut writer)
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
/// How far the kernel may take time now.
fn bound(clock: Clock, real_from: (Instant, f64)) -> Bound {
    match clock {
        Clock::Paused => Bound {
            until: f64::NEG_INFINITY,
            events: None,
        },
        Clock::Step(n) => Bound {
            until: f64::INFINITY,
            events: Some(n),
        },
        Clock::Fast(horizon) => Bound {
            until: horizon,
            events: None,
        },
        Clock::Real(factor) => Bound {
            until: real_from.1 + real_from.0.elapsed().as_secs_f64() * factor,
            events: None,
        },
    }
}

/// The store's time as `GET /time` shows it.
fn time_json(sim: &Sequencing, store: &Store) -> Json {
    let name = |a: Agent| match a {
        Agent::Process(p) => store.describe(AdtRef::Object(p)),
        Agent::Client(id) => format!("client {id}"),
        Agent::Main => "the main program".into(),
    };
    let due = (sim.due().into_iter())
        .map(|(t, a)| {
            Json::Object(BTreeMap::from([
                ("at".to_string(), Json::Number(t)),
                ("agent".to_string(), Json::String(name(a))),
            ]))
        })
        .collect();
    Json::Object(BTreeMap::from([
        ("now".to_string(), Json::Number(sim.now())),
        ("due".to_string(), Json::Array(due)),
    ]))
}

/// The executor: owns the store and its time; runs each ready session
/// for a slice in turn, round robin, and the kernel's processes between
/// them as the pace lets time move; waits for messages when none is
/// ready.
fn executor(
    mut store: Store,
    durable: Option<Durable>,
    rx: Receiver<Msg>,
    language: &'static (dyn Language + Sync),
    pace: Pace,
) {
    let mut clients: BTreeMap<u64, Client> = BTreeMap::new();
    let mut after = 0;
    let mut sim = Sequencing::kernel();
    let mut kernel = Session::kernel();
    let mut loaded = BTreeSet::new();
    // The modules earlier clients brought: their bodies serve still.
    for (hash, text) in store.modules() {
        if let Ok(unit) = Unit::read(text) {
            kernel.load(unit);
            loaded.insert(hash.clone());
        }
    }
    // The time and the processes as the last pause left them (decision 5).
    if let Some(text) = store.sequencing() {
        match Sequencing::restore(text) {
            Ok(s) => sim = s,
            Err(e) => eprintln!("the store's sequencing set does not read: {e}"),
        }
    }
    if let Some(text) = store.fibers()
        && let Err(e) = kernel.restore_fibers(text)
    {
        eprintln!("the store's processes do not read: {e}");
    }
    let mut clock = pace.clock;
    let mut real_from = (Instant::now(), 0.0);
    loop {
        let b = bound(clock, real_from);
        let in_time = |clients: &BTreeMap<u64, Client>, sim: &Sequencing| {
            (clients.iter()).any(|(&id, c)| !c.held && sim.is_engaged(Agent::Client(id)))
        };
        let kernel_ready = !in_time(&clients, &sim)
            && (kernel.in_process()
                || sim
                    .next_due()
                    .is_some_and(|t| t <= b.until && b.events != Some(0)));
        let ready = kernel_ready || clients.values().any(|c| !c.waiting && !c.held);
        let first = match ready {
            true => rx.try_recv().ok(),
            // Idle, the journal may still be catching up: it is looked at
            // now and then, to compact what the last turns left. At real
            // time, the next agent due wakes the executor.
            false => {
                let mut wait = IDLE;
                if let (Clock::Real(f), Some(t)) = (clock, sim.next_due()) {
                    let secs = ((t - b.until) / f).max(0.0);
                    wait = wait.min(Duration::from_secs_f64(secs));
                }
                match rx.recv_timeout(wait) {
                    Ok(m) => Some(m),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(d) = &durable {
                            d.maybe_compact(&store, pace.compact_at);
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
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
                } => match start(&acode, language, id) {
                    Ok((session, unit)) => {
                        // Kept by the store, once, and loaded into the
                        // kernel: its bodies serve the processes it starts.
                        match store.keep_module(&unit.text()) {
                            Ok(hash) => {
                                if loaded.insert(hash) {
                                    kernel.load(unit);
                                }
                            }
                            Err(e) => {
                                events.send(What::End(Some(e.0)));
                                continue;
                            }
                        }
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
                                held: false,
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
                    // Its processes stay; its own agent goes.
                    clients.remove(&id);
                    sim.forget(Agent::Client(id));
                }
                Msg::Dump(reply) => {
                    let _ = reply.send(store.dump());
                }
                Msg::Sessions(reply) => {
                    let _ = reply.send(clients.len());
                }
                Msg::Pace(set, reply) => {
                    if let Some(c) = set {
                        clock = c;
                        real_from = (Instant::now(), sim.now());
                    }
                    let _ = reply.send(clock.json().to_string());
                }
                Msg::Time(reply) => {
                    let _ = reply.send(time_json(&sim, &store).to_string());
                }
            }
        }
        // The next ready client after the last one served.
        let next = (clients.range(after + 1..).chain(clients.range(..=after)))
            .find(|(_, c)| !c.waiting && !c.held)
            .map(|(&id, _)| id);
        if let Some(id) = next {
            after = id;
            let c = clients.get_mut(&id).expect("a client");
            let turn = c.session.turn(
                &mut store,
                &mut sim,
                &mut c.wire,
                language,
                Some(pace.slice),
            );
            let gone = match turn {
                Turn::Yield | Turn::Idle | Turn::Due(_) => false,
                Turn::Input => {
                    c.waiting = true;
                    c.wire.events.send(What::WantsInput);
                    false
                }
                Turn::Held => {
                    c.held = true;
                    false
                }
                Turn::Done(_) => {
                    c.wire.events.send(What::End(None));
                    true
                }
                Turn::Failed(e) => {
                    c.wire.events.send(What::End(Some(e.to_string())));
                    true
                }
            };
            if gone {
                clients.remove(&id);
                sim.forget(Agent::Client(id));
            }
        }
        // The simulation, between the clients' turns: its active process
        // runs until it holds, never switched for another.
        let b = bound(clock, real_from);
        let kernel_ready = !in_time(&clients, &sim)
            && (kernel.in_process()
                || sim
                    .next_due()
                    .is_some_and(|t| t <= b.until && b.events != Some(0)));
        if kernel_ready {
            let mut io = Router {
                clients: (clients.iter())
                    .map(|(&id, c)| (id, c.wire.events.clone()))
                    .collect(),
                owner: None,
            };
            let turn =
                kernel.kernel_turn(&mut store, &mut sim, &mut io, language, Some(pace.slice), b);
            if let (Clock::Step(_), Some(left)) = (clock, kernel.events_left()) {
                clock = if left == 0 {
                    Clock::Paused
                } else {
                    Clock::Step(left)
                };
            }
            match turn {
                // Due, the client runs on, in time: the kernel waits for it.
                Turn::Due(id) => {
                    if let Some(c) = clients.get_mut(&id) {
                        c.held = false;
                    }
                }
                // A process's error ends it, and its client's run, as a
                // standalone run ends; with no client, the daemon's log
                // says it.
                Turn::Failed(e) => {
                    let owner = kernel.failed().and_then(|p| sim.owner(Agent::Process(p)));
                    match owner {
                        Some(Agent::Client(id)) if clients.contains_key(&id) => {
                            let c = clients.remove(&id).expect("a client");
                            c.wire.events.send(What::End(Some(e.to_string())));
                            sim.forget(Agent::Client(id));
                        }
                        _ => eprintln!("a process ends on an error: {e}"),
                    }
                }
                _ => {}
            }
        }
        if let Some(d) = &durable {
            d.maybe_compact(&store, pace.compact_at);
        }
    }
}

/// A client's session for the A-code in `text`, if this daemon can run
/// it, and the unit.
fn start(text: &str, language: &dyn Language, id: u64) -> Result<(Session, Unit), String> {
    let unit = Unit::read(text).map_err(|e| e.0)?;
    if unit.language != language.name() {
        return Err(format!(
            "A-code in {}, a language this daemon does not have",
            unit.language
        ));
    }
    Ok((Session::client(unit.clone(), id), unit))
}
