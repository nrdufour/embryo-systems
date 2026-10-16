//! The daemon (docs/architecture.md, decisions 3 and 8): programs run as
//! its clients' sessions give what they give standalone; sessions take
//! turns; the store is the daemon's, kept between clients.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_embryo-systems");

/// A daemon on a socket of its own, a memory store, killed when dropped.
struct Daemon {
    child: Child,
    socket: PathBuf,
}

impl Daemon {
    /// Time as fast as it goes, as a standalone run's.
    fn start(name: &str) -> Daemon {
        Daemon::with(name, &["--pace", "fast"])
    }

    /// A daemon with more arguments: a store, the journal's pace.
    fn with(name: &str, args: &[&str]) -> Daemon {
        let socket = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.sock"));
        let _ = fs::remove_file(&socket);
        let child = Command::new(BIN)
            .args([
                "daemon",
                "--slice",
                "50",
                "--socket",
                socket.to_str().unwrap(),
            ])
            .args(args)
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while UnixStream::connect(&socket).is_err() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the daemon does not listen"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        Daemon { child, socket }
    }

    fn socket(&self) -> &str {
        self.socket.to_str().unwrap()
    }

    /// A client: `embryo-systems run --connect` on `file`.
    fn client(&self, file: &str) -> Command {
        let mut c = Command::new(BIN);
        c.args(["run", "--connect", self.socket(), file]);
        c
    }

    /// What `GET path` answers.
    fn get(&self, path: &str) -> String {
        let mut s = UnixStream::connect(&self.socket).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\n\r\n").unwrap();
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        text.split_once("\r\n\r\n").unwrap().1.to_string()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Output, then the error as the corpus's `.out` shows it, without the
/// file's name.
fn outcome(out: std::process::Output, file: &str) -> String {
    let stderr = String::from_utf8(out.stderr).unwrap();
    let mut text = String::from_utf8(out.stdout).unwrap();
    text += &stderr.replace(&format!("{file}:"), "");
    text
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/adt81/tests/corpus/run")
}

fn write(dir: &str, name: &str, source: &str) -> String {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    fs::write(&file, source).unwrap();
    file.to_str().unwrap().to_string()
}

#[test]
fn the_corpus_runs_as_sessions() {
    let root = corpus();
    let mut paths = Vec::new();
    for dir in ["", "errors", "store", "store/errors"] {
        for e in fs::read_dir(root.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "adt") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    let mut differ = Vec::new();
    for path in &paths {
        // A daemon each: every program expects a store of its own.
        let daemon = Daemon::start("corpus");
        let file = path.to_str().unwrap();
        let input = fs::read(path.with_extension("in")).unwrap_or_default();
        let mut child = daemon
            .client(file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let got = outcome(child.wait_with_output().unwrap(), file);
        let expected = fs::read_to_string(path.with_extension("out")).unwrap();
        if got != expected {
            differ.push(format!(
                "{file}\n  expected: {expected:?}\n  got:      {got:?}"
            ));
        }
    }
    assert!(differ.is_empty(), "\n{}", differ.join("\n"));
}

#[test]
fn sessions_take_turns_and_leave() {
    let daemon = Daemon::start("turns");
    let forever = write(
        "daemon-turns",
        "forever.adt",
        "program forever ;\nlocal\n        n : integer init 0 ;\nbegin\n        print \"started\" ;\n        loop while true do\n                n := n + 1\n        endloop\nend ;\n",
    );
    let short = write(
        "daemon-turns",
        "short.adt",
        "program short ;\nbegin\n        print \"short\"\nend ;\n",
    );
    let mut long = daemon
        .client(&forever)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(long.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    assert_eq!(first, "started\n");
    // The loop never ends: the short program runs in the turns between.
    let out = daemon.client(&short).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "short\n");
    assert_eq!(daemon.get("/sessions").lines().count(), 1);
    // Its client gone, the session goes.
    long.kill().unwrap();
    long.wait().unwrap();
    let start = Instant::now();
    while !daemon.get("/sessions").is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the session outlives its client"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = daemon.client(&short).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "short\n");
}

#[test]
fn the_store_is_the_daemons() {
    let daemon = Daemon::start("store");
    let keeps = write(
        "daemon-store",
        "keeps.adt",
        "program keeps ;\nbegin\n        create \"c\" ; create \"c\" . \"a\" ;\n        create \"o\" [ \"c\" ] ;\n        let \"o\" [ \"c\" ] . \"a\" := 7\nend ;\n",
    );
    let finds = write(
        "daemon-store",
        "finds.adt",
        "program finds ;\nbegin\n        print \"o\" [ \"c\" ] . \"a\"\nend ;\n",
    );
    assert!(daemon.client(&keeps).status().unwrap().success());
    let out = daemon.client(&finds).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "7\n");
    // The model, then the modules the two clients brought, kept by hash.
    let dump = daemon.get("/store");
    assert!(
        dump.starts_with("class c\n  attribute a\n  object o\n    a = 7\nmodule "),
        "{dump}"
    );
    assert_eq!(dump.matches("module ").count(), 2, "{dump}");
    // An .acode file and a built program connect too.
    let acode = write("daemon-store", "finds.acode", "");
    let text = Command::new(BIN)
        .args(["acode", &finds])
        .output()
        .unwrap()
        .stdout;
    fs::write(&acode, text).unwrap();
    let out = Command::new(BIN)
        .args(["exec", "--connect", daemon.socket(), &acode])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "7\n");
    let built = write("daemon-store", "finds", "");
    assert!(
        Command::new(BIN)
            .args(["build", &finds, "-o", &built])
            .status()
            .unwrap()
            .success()
    );
    let out = Command::new(&built)
        .args(["--connect", daemon.socket()])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "7\n");
}

fn store_dir(name: &str) -> String {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    dir.to_str().unwrap().to_string()
}

#[test]
fn a_client_hears_nothing_before_its_sync() {
    let dir = store_dir("daemon-slow-store");
    let daemon = Daemon::with("slow", &["--store", &dir, "--sync-delay", "400"]);
    let keeps = write(
        "daemon-slow",
        "keeps.adt",
        "program keeps ;\nbegin\n        create \"c\" ;\n        print \"kept\"\nend ;\n",
    );
    let start = Instant::now();
    let out = daemon.client(&keeps).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "kept\n");
    // The class's transaction syncs in no less than the delay; the
    // output that follows it waited.
    assert!(
        start.elapsed() >= Duration::from_millis(400),
        "{:?}",
        start.elapsed()
    );
    let journal = daemon.get("/journal");
    assert!(
        // The unit kept, then the class.
        journal.contains("\"submitted\":2,\"synced\":2"),
        "{journal}"
    );
}

#[test]
fn the_journal_compacts_and_survives_a_kill() {
    let dir = store_dir("daemon-compact-store");
    let counts = write(
        "daemon-compact",
        "counts.adt",
        "program counts ;\nlocal\n        i : integer ;\nbegin\n        create \"c\" ; create \"c\" . \"n\" ; create \"o\" [ \"c\" ] ;\n        loop for i : 1 to 300 do\n                let \"o\" [ \"c\" ] . \"n\" := i\n        endloop ;\n        print \"counted\"\nend ;\n",
    );
    let finds = write(
        "daemon-compact",
        "finds.adt",
        "program finds ;\nbegin\n        print \"o\" [ \"c\" ] . \"n\"\nend ;\n",
    );
    {
        let daemon = Daemon::with("compact", &["--store", &dir, "--compact-at", "50"]);
        let out = daemon.client(&counts).output().unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "counted\n");
        // The journal catches up, then compacts what the run left.
        let start = Instant::now();
        let lines = |j: &str| {
            let at = j.find("\"lines\":").expect("lines") + "\"lines\":".len();
            let n: String = j[at..].chars().take_while(char::is_ascii_digit).collect();
            n.parse::<u64>().unwrap()
        };
        loop {
            let journal = daemon.get("/journal");
            if !journal.contains("\"compactions\":0") && lines(&journal) < 100 {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "{journal}");
            std::thread::sleep(Duration::from_millis(20));
        }
        // Dropped: killed, SIGKILL, with no chance to tidy up.
    }
    let lines = fs::read_to_string(Path::new(&dir).join("journal"))
        .unwrap()
        .lines()
        .count();
    assert!(lines < 100, "{lines} lines in the journal");
    let daemon = Daemon::with("compact", &["--store", &dir]);
    let out = daemon.client(&finds).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "300\n");
}

/// A client's output, failing the test if it runs past `secs`.
fn within(mut child: Child, secs: u64) -> String {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut out = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut out)
                .unwrap();
            let mut err = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            assert!(status.success(), "{out}{err}");
            return out;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            panic!("the client is still running after {secs} s");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn spawn(mut c: Command) -> Child {
    c.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

impl Daemon {
    fn post(&self, path: &str, body: &str) -> String {
        let mut s = UnixStream::connect(&self.socket).unwrap();
        write!(
            s,
            "POST {path} HTTP/1.1\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        text.split_once("\r\n\r\n").unwrap().1.to_string()
    }

    /// The store's time, `GET /time`'s `now`.
    fn now(&self) -> f64 {
        let t = self.get("/time");
        let at = t.find("\"now\":").expect("now") + "\"now\":".len();
        let n: String = t[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e')
            .collect();
        n.parse().unwrap_or_else(|_| panic!("{t}"))
    }
}

/// A process class "w" whose body, a loop, sets THIS's "v" to `version`
/// and holds for 1, and `start` with objects `objects` activated.
fn simulation(version: u32, objects: &[&str]) -> String {
    let mut s = String::from("program start ;\nbegin\n");
    if version == 1 {
        s += "        create \"w\" ; include \"w\" into \"process\" ; create \"w\" . \"v\" ;\n";
    }
    for o in objects {
        s += &format!("        create \"{o}\" [ \"w\" ] ;\n        activate \"{o}\" [ \"w\" ] ;\n");
    }
    s += "        print \"started\"\nend ;\n\n";
    s += &format!(
        "coroutine \"w\" ;\nbegin\n        loop while true do\n                let this . \"v\" := {version} ;\n                hold for 1\n        endloop\nend ;\n"
    );
    s
}

#[test]
fn processes_outlive_their_client() {
    let daemon = Daemon::start("outlive");
    let start = write("daemon-outlive", "start.adt", &simulation(1, &["w1"]));
    assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
    // Gone, its client; the process runs on, in the store's time.
    let t = Instant::now();
    while daemon.now() < 50.0 {
        assert!(t.elapsed() < Duration::from_secs(20), "time stands still");
        std::thread::sleep(Duration::from_millis(10));
    }
    let reads = write(
        "daemon-outlive",
        "reads.adt",
        // In time first, so as to read the process between its holds.
        "program reads ;\nbegin\n        hold for 0 ;\n        print \"w1\" [ \"w\" ] . \"state\", \"w1\" [ \"w\" ] . \"v\", time > 49\nend ;\n",
    );
    assert_eq!(within(spawn(daemon.client(&reads)), 20), "HOLD 1 TRUE\n");
}

#[test]
fn a_hold_waits_for_the_pace() {
    let daemon = Daemon::with("paced", &["--pace", "paused"]);
    let holds = write(
        "daemon-paced",
        "holds.adt",
        "program holds ;\nbegin\n        print \"before\" ;\n        hold for 5 ;\n        print \"after\", time\nend ;\n",
    );
    let client = spawn(daemon.client(&holds));
    // Paused: the client waits in the set.
    let t = Instant::now();
    while !daemon.get("/time").contains("client") {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "{}",
            daemon.get("/time")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    assert!(daemon.get("/time").contains("client"), "it went on, paused");
    assert_eq!(
        daemon.post("/pace", "{\"pace\": \"fast\"}"),
        "{\"pace\":\"fast\"}\n"
    );
    assert_eq!(within(client, 20), "before\nafter 5.0\n");
}

#[test]
fn a_step_moves_one_event() {
    let daemon = Daemon::with("step", &["--pace", "paused"]);
    let start = write("daemon-step", "start.adt", &simulation(1, &["w1"]));
    assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
    assert_eq!(daemon.now(), 0.0);
    for expected in [0.0, 1.0, 2.0] {
        daemon.post("/pace", "{\"pace\": \"step\"}");
        let t = Instant::now();
        while !daemon.get("/pace").contains("paused") {
            assert!(
                t.elapsed() < Duration::from_secs(20),
                "{}",
                daemon.get("/pace")
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(daemon.now(), expected);
    }
}

#[test]
fn real_time_moves_with_the_clock() {
    let daemon = Daemon::with("real", &["--pace", "real", "--factor", "1000"]);
    let start = write("daemon-real", "start.adt", &simulation(1, &["w1"]));
    assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
    std::thread::sleep(Duration::from_millis(300));
    let now = daemon.now();
    // 0.3 s at a thousand times: about 300, never ahead of the clock.
    assert!((100.0..=1000.0).contains(&now), "{now}");
}

#[test]
fn a_new_body_serves_new_activations() {
    let daemon = Daemon::start("versions");
    let first = write("daemon-versions", "first.adt", &simulation(1, &["w1"]));
    assert_eq!(within(spawn(daemon.client(&first)), 20), "started\n");
    let second = write("daemon-versions", "second.adt", &simulation(2, &["w2"]));
    assert_eq!(within(spawn(daemon.client(&second)), 20), "started\n");
    // The first process finishes in the body it started in.
    let reads = write(
        "daemon-versions",
        "reads.adt",
        "program reads ;\nbegin\n        hold for 3 ;\n        print \"w1\" [ \"w\" ] . \"v\", \"w2\" [ \"w\" ] . \"v\"\nend ;\n",
    );
    assert_eq!(within(spawn(daemon.client(&reads)), 20), "1 2\n");
}

/// A process class "w" whose body counts in a local variable, keeps the
/// count in THIS's "n", works a while (so that a kill falls in the middle
/// of a step) and holds for 1: `n` is its clock, whenever it is held, if
/// no step ever runs twice nor is lost.
const COUNTER: &str = "program start ;\nbegin\n        create \"w\" ; include \"w\" into \"process\" ; create \"w\" . \"n\" ;\n        create \"w1\" [ \"w\" ] ;\n        activate \"w1\" [ \"w\" ] ;\n        print \"started\"\nend ;\n\ncoroutine \"w\" ;\nlocal\n        i, j, k : integer init 0 ;\nbegin\n        loop while true do\n                i := i + 1 ;\n                let this . \"n\" := i ;\n                loop for j : 1 to 2000 do k := k + j endloop ;\n                hold for 1\n        endloop\nend ;\n";

const READS: &str = "program reads ;\nbegin\n        print \"w1\" [ \"w\" ] . \"n\", \"w1\" [ \"w\" ] . \"clock\"\nend ;\n";

/// `n` and the clock, as READS prints them.
fn count(daemon: &Daemon, reads: &str) -> (f64, f64) {
    let out = within(spawn(daemon.client(reads)), 20);
    let mut words = out.split_whitespace().map(|w| w.parse::<f64>().unwrap());
    (words.next().unwrap(), words.next().unwrap())
}

#[test]
fn processes_resume_where_they_were() {
    let dir = store_dir("daemon-exact-store");
    let start = write("daemon-exact", "start.adt", COUNTER);
    let reads = write("daemon-exact", "reads.adt", READS);
    {
        // As fast as it goes: the kill falls in the middle of a step.
        let daemon = Daemon::with("exact", &["--store", &dir, "--pace", "fast"]);
        assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
        let t = Instant::now();
        while daemon.now() < 20.0 {
            assert!(t.elapsed() < Duration::from_secs(20), "time stands still");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Killed in the middle of whatever it does.
    }
    let daemon = Daemon::with("exact", &["--store", &dir, "--pace", "paused"]);
    let (n, clock) = count(&daemon, &reads);
    assert!(n >= 15.0, "{n} counted");
    assert_eq!(n, clock, "a step run twice, or lost");
    // It counts on from where it was, its local variable kept.
    daemon.post(
        "/pace",
        &format!("{{\"pace\": \"fast\", \"horizon\": {}}}", clock + 5.0),
    );
    let t = Instant::now();
    while daemon.now() < clock + 5.0 {
        assert!(t.elapsed() < Duration::from_secs(20), "time stands still");
        std::thread::sleep(Duration::from_millis(5));
    }
    // The event due at the horizon runs too: held again, one later.
    let t = Instant::now();
    loop {
        let (m, later) = count(&daemon, &reads);
        if (m, later) == (n + 6.0, clock + 6.0) {
            break;
        }
        assert!(later <= clock + 6.0, "past the horizon: {m} at {later}");
        assert!(t.elapsed() < Duration::from_secs(20), "{m} at {later}");
    }
}

#[test]
fn an_activation_outlives_a_crash() {
    let dir = store_dir("daemon-activation-store");
    let start = write("daemon-activation", "start.adt", COUNTER);
    {
        let daemon = Daemon::with("activation", &["--store", &dir, "--pace", "paused"]);
        assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
    }
    let daemon = Daemon::with("activation", &["--store", &dir, "--pace", "paused"]);
    let time = daemon.get("/time");
    assert!(time.contains("\"agent\":\"w1[w]\""), "{time}");
}

/// A subscriber to the feed: its frames, as they come.
struct Feed {
    reader: std::io::BufReader<UnixStream>,
}

impl Feed {
    fn open(daemon: &Daemon) -> Feed {
        let stream = UnixStream::connect(&daemon.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut writer = stream.try_clone().unwrap();
        hbds_http::upgrade_request("/events")
            .write(&mut writer)
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let r = hbds_http::Response::read(&mut reader).unwrap();
        assert_eq!(r.status, 101);
        Feed { reader }
    }

    /// The next frame.
    fn next(&mut self) -> String {
        match hbds_http::read_message(&mut self.reader).expect("a frame in time") {
            hbds_http::Message::Text(t) => t,
            m => panic!("{m:?}"),
        }
    }

    /// Frames until one contains `wanted`, which it gives.
    fn until(&mut self, wanted: &str) -> String {
        loop {
            let f = self.next();
            if f.contains(wanted) {
                return f;
            }
        }
    }
}

#[test]
fn the_feed_tells_what_happens() {
    let dir = store_dir("daemon-feed-store");
    let daemon = Daemon::with(
        "feed",
        &["--store", &dir, "--pace", "paused", "--sync-delay", "300"],
    );
    let mut feed = Feed::open(&daemon);
    let begun = Instant::now();
    let keeps = write(
        "daemon-feed",
        "keeps.adt",
        "program keeps ;\nbegin\n        create \"c\" ;\n        print \"kept\"\nend ;\n",
    );
    let client = spawn(daemon.client(&keeps));
    // The unit kept, the session started, its change, its end.
    feed.until("{\"module\":");
    assert!(feed.until("started").contains("\"session\":"));
    assert_eq!(
        feed.until("change"),
        "{\"change\":\"class 1 \\\"c\\\" class\"}"
    );
    // Not before it is on disk: one slowed sync at least (the unit's and
    // the class's may share one).
    assert!(
        begun.elapsed() >= Duration::from_millis(300),
        "{:?}",
        begun.elapsed()
    );
    assert!(
        feed.until("ended")
            .starts_with("{\"ended\":null,\"session\":")
    );
    assert_eq!(within(client, 20), "kept\n");
    // A process whose client left prints to the feed; time and pace go by.
    let start = write("daemon-feed", "start.adt", &simulation(1, &["w1"]));
    assert_eq!(within(spawn(daemon.client(&start)), 20), "started\n");
    let printer = write(
        "daemon-feed",
        "printer.adt",
        "program printer ;\nbegin\n        create \"p\" ; include \"p\" into \"process\" ;\n        create \"p1\" [ \"p\" ] ; activate \"p1\" [ \"p\" ]\nend ;\n\ncoroutine \"p\" ;\nbegin\n        print \"tick\", time ;\n        hold for 1 ;\n        print \"tock\", time\nend ;\n",
    );
    assert_eq!(within(spawn(daemon.client(&printer)), 20), "");
    daemon.post("/pace", "{\"pace\": \"fast\", \"horizon\": 3}");
    assert_eq!(
        feed.until("pace"),
        "{\"pace\":{\"horizon\":3,\"pace\":\"fast\"}}"
    );
    assert!(feed.until("tick").contains("\"output\":\"tick 0.0\\n\""));
    assert!(feed.until("tock").contains("\"output\":\"tock 1.0\\n\""));
    // The time as each kernel turn leaves it, up to the horizon.
    feed.until("{\"time\":3}");
}

#[test]
fn an_administrator_kills_a_session() {
    let daemon = Daemon::with("kill", &["--pace", "paused"]);
    // Held, in the set: killed, it leaves the set too.
    let holds = write(
        "daemon-kill",
        "holds.adt",
        "program holds ;\nbegin\n        hold for 5 ;\n        print \"never\"\nend ;\n",
    );
    let held = spawn(daemon.client(&holds));
    let t = Instant::now();
    while !daemon.get("/time").contains("client") {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "{}",
            daemon.get("/time")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let id = daemon.get("/sessions").trim().to_string();
    assert_eq!(daemon.post(&format!("/sessions/{id}/kill"), ""), "killed\n");
    let t = Instant::now();
    let mut held = held;
    let status = loop {
        if let Some(status) = held.try_wait().unwrap() {
            break status;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "the killed client runs on"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
    assert!(
        !daemon.get("/time").contains("client"),
        "{}",
        daemon.get("/time")
    );
    let forever = write(
        "daemon-kill",
        "forever.adt",
        "program forever ;\nlocal\n        n : integer init 0 ;\nbegin\n        print \"started\" ;\n        loop while true do\n                n := n + 1\n        endloop\nend ;\n",
    );
    let mut client = daemon
        .client(&forever)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(client.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let id = daemon.get("/sessions").trim().to_string();
    assert_eq!(daemon.post(&format!("/sessions/{id}/kill"), ""), "killed\n");
    let status = client.wait().unwrap();
    let mut err = String::new();
    client
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(
        !status.success() && err.contains("killed by an administrator"),
        "{err}"
    );
    assert_eq!(
        daemon.post(&format!("/sessions/{id}/kill"), ""),
        "no such session\n"
    );
    assert!(daemon.get("/sessions").is_empty());
    let short = write(
        "daemon-kill",
        "short.adt",
        "program short ;\nbegin\n        print \"short\"\nend ;\n",
    );
    assert_eq!(within(spawn(daemon.client(&short)), 20), "short\n");
}
