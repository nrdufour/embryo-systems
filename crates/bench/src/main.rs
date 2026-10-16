//! The benchmarks (docs/architecture.md, decision 12): each scenario of
//! `bench/scenarios` run on a ladder of sizes, its input generated here,
//! its output checked, the median of a few runs kept with the work counted
//! beside it. The work is the same on every host, and must match
//! `bench/work.txt` exactly; the times are compared against
//! `bench/<host>.txt` when this host has one, and one more than 15% off
//! fails, slower or faster. Either way a change is recorded on purpose:
//! `--record` writes both files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use adt81::compile::{Adt81, compile};
use adt81::diagnostic::Severity;
use adt81::interp::{self, Io};
use adt81::lower;
use hbds_gtad::{Store, Work};

/// Runs of each size, after one to warm up; the median is kept.
const RUNS: usize = 5;
/// How far a time may move before the gate fails.
const MARGIN: f64 = 0.15;
/// Times below this are printed but not gated: noise is most of them.
const FLOOR_MS: f64 = 100.0;

const TARGET: &str = "\
# The print's scale, the target (decision 12): a million objects for the
# store and functors, ten thousand rules over a hundred thousand facts for
# chav, a hundred thousand processes and a million events for simulation.";

struct Scenario {
    name: &'static str,
    /// The program in `bench/scenarios`.
    program: &'static str,
    /// Run compiled to A-code, on the VM, instead of by the reference.
    vm: bool,
    sizes: &'static [u64],
    input: fn(u64) -> String,
    expected: fn(u64) -> String,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "store",
        program: "store",
        vm: false,
        sizes: &[500, 1000, 2000, 4000],
        input: |n| format!("{n}\n{}", names("o", n)),
        expected: |n| format!("{n} {}\n", triangle(n)),
    },
    Scenario {
        name: "lookup",
        program: "lookup",
        vm: false,
        sizes: &[500, 1000, 2000, 4000],
        input: |n| {
            let back: String = (1..=n).rev().map(|i| format!("o{i}\n")).collect();
            format!("{n}\n{}{back}", names("o", n))
        },
        expected: |n| format!("{}\n", triangle(n)),
    },
    Scenario {
        name: "functors",
        program: "functors",
        vm: false,
        sizes: &[125, 250, 500, 1000],
        input: |n| {
            let mut s = format!("{n}\n{}", names("p", n));
            for i in 1..=n {
                writeln!(s, "s{i}\np{i}\np{}", i % n + 1).unwrap();
            }
            s
        },
        expected: |n| format!("{n} {}\n", triangle(n)),
    },
    Scenario {
        name: "chav",
        program: "chav",
        vm: false,
        sizes: &[125, 250, 500, 1000],
        input: |n| {
            let mut s = format!("{n}\n{}", names("s", n));
            for i in 1..=n {
                writeln!(s, "r{i}").unwrap();
                writeln!(s, "\"s{i}\" [ \"signal\" ] . \"v\" = 1").unwrap();
                writeln!(s, "let \"s{i}\" [ \"signal\" ] . \"w\" := 1").unwrap();
            }
            s
        },
        expected: |n| format!("{n}\n"),
    },
    Scenario {
        name: "simulation",
        program: "simulation",
        vm: false,
        sizes: &[50, 100, 200, 400],
        input: |n| format!("{n}\n{}", names("w", n)),
        expected: |n| format!("21.0 {}\n", 20 * n),
    },
    Scenario {
        name: "values",
        program: "values",
        vm: false,
        sizes: &[32_000, 64_000, 128_000, 256_000],
        input: |n| format!("{n}\n"),
        expected: |n| format!("{}\n", n * n),
    },
    Scenario {
        name: "values-vm",
        program: "values",
        vm: true,
        sizes: &[32_000, 64_000, 128_000, 256_000],
        input: |n| format!("{n}\n"),
        expected: |n| format!("{}\n", n * n),
    },
    Scenario {
        name: "language",
        program: "language",
        vm: false,
        sizes: &[125_000, 250_000, 500_000, 1_000_000],
        input: |n| format!("{n}\n"),
        expected: |n| {
            let t: i64 = (1..=n as i64).map(|i| i * i / 3 - i / 2).sum();
            let u: u64 = (1..=n).filter(|e| *e > n / 2).sum();
            format!("{t} {n} {u}\n")
        },
    },
    Scenario {
        name: "language-vm",
        program: "language",
        vm: true,
        sizes: &[125_000, 250_000, 500_000, 1_000_000],
        input: |n| format!("{n}\n"),
        expected: |n| {
            let t: i64 = (1..=n as i64).map(|i| i * i / 3 - i / 2).sum();
            let u: u64 = (1..=n).filter(|e| *e > n / 2).sum();
            format!("{t} {n} {u}\n")
        },
    },
];

/// `prefix1` to `prefixN`, a line each.
fn names(prefix: &str, n: u64) -> String {
    (1..=n).map(|i| format!("{prefix}{i}\n")).collect()
}

fn triangle(n: u64) -> u64 {
    n * (n + 1) / 2
}

/// Stdin from a string, stdout into one.
struct Pipe {
    lines: std::vec::IntoIter<String>,
    out: String,
}

impl hbds_vm::Io for Pipe {
    fn read_line(&mut self) -> Option<String> {
        self.lines.next()
    }

    fn write(&mut self, text: &str) {
        self.out.push_str(text);
    }
}

impl Io for Pipe {
    fn read_line(&mut self) -> Option<String> {
        self.lines.next()
    }

    fn write(&mut self, text: &str) {
        self.out.push_str(text);
    }
}

/// One size's result.
#[derive(Debug, Clone, PartialEq)]
struct Measure {
    ms: f64,
    statements: u64,
    work: Work,
}

fn measure(s: &Scenario, source: &str, n: u64) -> Result<Measure, String> {
    let program = lower::parse_program(source).map_err(|e| e.to_string())?;
    let errors: Vec<String> = (adt81::check(&program).iter())
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.to_string())
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let input = (s.input)(n);
    let expected = (s.expected)(n);
    let unit = match s.vm {
        true => Some(compile(&program).map_err(|e| format!("not yet on the VM: {}", e.what))?),
        false => None,
    };
    let mut runs = Vec::new();
    for run in 0..=RUNS {
        let lines: Vec<String> = input.lines().map(String::from).collect();
        let pipe = Arc::new(Mutex::new(Pipe {
            lines: lines.into_iter(),
            out: String::new(),
        }));
        let store = Arc::new(Mutex::new(Store::memory()));
        let start = Instant::now();
        let statements = match &unit {
            Some(unit) => {
                let mut io = pipe.lock().unwrap();
                let mut store = store.lock().unwrap();
                hbds_vm::run(unit, &mut store, &mut *io, &Adt81, None).map_err(|e| e.to_string())?
            }
            None => interp::run_counted(program.clone(), pipe.clone(), store.clone())
                .map_err(|e| e.to_string())?,
        };
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let out = std::mem::take(&mut pipe.lock().unwrap().out);
        if out != expected {
            return Err(format!("printed {out:?}, where {expected:?} was expected"));
        }
        let work = store.lock().unwrap().work();
        if run == 0 {
            continue;
        }
        runs.push(Measure {
            ms,
            statements,
            work,
        });
    }
    if runs
        .windows(2)
        .any(|w| (w[0].statements, w[0].work) != (w[1].statements, w[1].work))
    {
        return Err("the work counted differs from one run to the next".into());
    }
    runs.sort_by(|a, b| a.ms.total_cmp(&b.ms));
    Ok(runs.swap_remove(RUNS / 2))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

fn host() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn time_header() -> String {
    format!(
        "{:<10} {:>8} {:>8} {:>6}",
        "scenario", "size", "ms", "growth"
    )
}

fn work_header() -> String {
    format!(
        "{:>10} {:>8} {:>8} {:>11} {:>11}",
        "statements", "changes", "lookups", "scanned", "copied"
    )
}

fn line(name: &str, n: u64, m: &Measure, growth: Option<f64>) -> String {
    format!("{} {}", time_line(name, n, m, growth), work_columns(m))
}

fn time_line(name: &str, n: u64, m: &Measure, growth: Option<f64>) -> String {
    let growth = growth.map_or("-".into(), |g| format!("x{g:.1}"));
    format!("{name:<10} {n:>8} {:>8.1} {growth:>6}", m.ms)
}

fn work_columns(m: &Measure) -> String {
    let w = &m.work;
    format!(
        "{:>10} {:>8} {:>8} {:>11} {:>11}",
        m.statements, w.changes, w.lookups, w.scanned, w.copied
    )
}

type Key = (String, u64);

/// A results file's lines, by scenario and size, their fields after those
/// two; `width` fields to a line.
fn parse(text: &str, width: usize) -> Result<BTreeMap<Key, Vec<String>>, String> {
    let mut out = BTreeMap::new();
    for (i, l) in text.lines().enumerate() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.is_empty() || f[0].starts_with('#') || f[0] == "scenario" {
            continue;
        }
        let bad = || format!("line {}: {l:?}", i + 1);
        let n = f.get(1).and_then(|n| n.parse().ok()).ok_or_else(bad)?;
        if f.len() != width {
            return Err(bad());
        }
        let rest = f[2..].iter().map(|s| s.to_string()).collect();
        out.insert((f[0].to_string(), n), rest);
    }
    Ok(out)
}

/// What changed in the work, said briefly; empty when nothing did.
fn work_delta(was: &[String], is: &Measure) -> String {
    let names = ["statements", "changes", "lookups", "scanned", "copied"];
    let now = work_columns(is);
    (names.iter().zip(was).zip(now.split_whitespace()))
        .filter(|((_, a), b)| a.as_str() != *b)
        .map(|((k, a), b)| format!("{k} {a} -> {b}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A results file, or none; a file that does not parse is an error.
fn read(path: &PathBuf, width: usize) -> Result<Option<BTreeMap<Key, Vec<String>>>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, width)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let record = args.iter().any(|a| a == "--record");
    let only: Vec<&String> = args.iter().filter(|a| *a != "--record").collect();
    if record && !only.is_empty() {
        eprintln!("--record measures every scenario");
        return ExitCode::FAILURE;
    }
    let host = host();
    let times_file = root().join(format!("{host}.txt"));
    let work_file = root().join("work.txt");
    let (times, work) = match (read(&times_file, 4), read(&work_file, 7)) {
        _ if record => (None, None),
        (Ok(t), Ok(w)) => (t, w),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let intro = format!(
        "# Benchmarks on {host}: the median of {RUNS} runs, in milliseconds; growth\n\
         # is a size's time over the one before (x2 linear, x4 quadratic). The\n\
         # work done is in bench/work.txt.\n\
         {TARGET}\n\n{}\n",
        time_header()
    );
    let mut time_table = intro;
    let mut work_table = format!(
        "# The work each benchmark does, the same on every host, gated exactly:\n\
         # statements executed, and the store's changes, lookups, items its\n\
         # lookups scanned and entries its maps copied on write.\n\n\
         {:<10} {:>8} {}\n",
        "scenario",
        "size",
        work_header()
    );
    println!("{} {}", time_header(), work_header());
    let mut failures = Vec::new();
    // What recording again settles: a time off, a change of work, a size
    // not recorded.
    let mut stale = false;
    for s in SCENARIOS {
        if !only.is_empty() && !only.iter().any(|o| *o == s.name) {
            continue;
        }
        let path = root().join("scenarios").join(format!("{}.adt", s.program));
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let mut before: Option<f64> = None;
        for &n in s.sizes {
            let m = match measure(s, &source, n) {
                Ok(m) => m,
                Err(e) => {
                    failures.push(format!("{} {n}: {e}", s.name));
                    break;
                }
            };
            let growth = before.map(|b| m.ms / b);
            before = Some(m.ms);
            println!("{}", line(s.name, n, &m, growth));
            writeln!(time_table, "{}", time_line(s.name, n, &m, growth)).unwrap();
            writeln!(work_table, "{:<10} {n:>8} {}", s.name, work_columns(&m)).unwrap();
            let key = (s.name.to_string(), n);
            if let Some(work) = &work {
                match work.get(&key) {
                    None => {
                        failures.push(format!("{} {n}: not in bench/work.txt", s.name));
                        stale = true;
                    }
                    Some(was) => {
                        let delta = work_delta(was, &m);
                        if !delta.is_empty() {
                            failures.push(format!("{} {n}: the work changed: {delta}", s.name));
                            stale = true;
                        }
                    }
                }
            }
            let Some(times) = &times else { continue };
            let Some(was) = times.get(&key) else {
                failures.push(format!("{} {n}: not in bench/{host}.txt", s.name));
                stale = true;
                continue;
            };
            let was: f64 = was[0].parse().unwrap_or(f64::NAN);
            let ratio = m.ms / was;
            if was >= FLOOR_MS && (ratio - 1.0).abs() > MARGIN {
                stale = true;
                let way = if ratio > 1.0 { "slower" } else { "faster" };
                failures.push(format!(
                    "{} {n}: {:.1} ms, was {was:.1}: {:.0}% {way}",
                    s.name,
                    m.ms,
                    (ratio - 1.0).abs() * 100.0
                ));
            }
        }
    }
    if record {
        for (path, table) in [(&times_file, &time_table), (&work_file, &work_table)] {
            if let Err(e) = std::fs::write(path, table) {
                failures.push(format!("{}: {e}", path.display()));
            }
        }
        println!("\nrecorded in bench/{host}.txt and bench/work.txt");
    } else if times.is_none() {
        println!("\nno bench/{host}.txt: the times printed, not compared");
    }
    if failures.is_empty() {
        return ExitCode::SUCCESS;
    }
    eprintln!();
    for f in &failures {
        eprintln!("{f}");
    }
    if stale {
        eprintln!(
            "a change on purpose: `just bench-record`, and commit bench/work.txt and \
             bench/{host}.txt; the runner's times are those its CI run prints"
        );
    }
    ExitCode::FAILURE
}
