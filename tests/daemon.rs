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
    fn start(name: &str) -> Daemon {
        Daemon::with(name, &[])
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
    assert_eq!(daemon.get("/sessions"), "1\n");
    // Its client gone, the session goes.
    long.kill().unwrap();
    long.wait().unwrap();
    let start = Instant::now();
    while daemon.get("/sessions") != "0\n" {
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
    assert_eq!(
        daemon.get("/store"),
        "class c\n  attribute a\n  object o\n    a = 7\n"
    );
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
        journal.contains("\"submitted\":1,\"synced\":1"),
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
