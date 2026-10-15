//! The benchmarks (docs/architecture.md, decision 12): each scenario of
//! `bench/scenarios` run on a ladder of sizes, its input generated here,
//! its output checked, the median of a few runs kept with the work counted
//! beside it. Compared against `bench/<host>.txt` when this host has one:
//! a time more than 15% off fails, slower or faster, so that a change of
//! speed is recorded on purpose. `--record` writes the file.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use adt81::diagnostic::Severity;
use adt81::interp::{self, Io};
use adt81::lower;
use hbds_gtad::{Store, Work};

/// Runs of each size; the median is kept.
const RUNS: usize = 3;
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
    sizes: &'static [u64],
    input: fn(u64) -> String,
    expected: fn(u64) -> String,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "store",
        sizes: &[500, 1000, 2000, 4000],
        input: |n| format!("{n}\n{}", names("o", n)),
        expected: |n| format!("{n} {}\n", triangle(n)),
    },
    Scenario {
        name: "lookup",
        sizes: &[500, 1000, 2000, 4000],
        input: |n| {
            let back: String = (1..=n).rev().map(|i| format!("o{i}\n")).collect();
            format!("{n}\n{}{back}", names("o", n))
        },
        expected: |n| format!("{}\n", triangle(n)),
    },
    Scenario {
        name: "functors",
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
        sizes: &[50, 100, 200, 400],
        input: |n| format!("{n}\n{}", names("w", n)),
        expected: |n| format!("21.0 {}\n", 20 * n),
    },
    Scenario {
        name: "language",
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
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let lines: Vec<String> = input.lines().map(String::from).collect();
        let pipe = Arc::new(Mutex::new(Pipe {
            lines: lines.into_iter(),
            out: String::new(),
        }));
        let store = Arc::new(Mutex::new(Store::memory()));
        let start = Instant::now();
        let statements = interp::run_counted(program.clone(), pipe.clone(), store.clone())
            .map_err(|e| e.to_string())?;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let out = std::mem::take(&mut pipe.lock().unwrap().out);
        if out != expected {
            return Err(format!("printed {out:?}, where {expected:?} was expected"));
        }
        let work = store.lock().unwrap().work();
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

fn header() -> String {
    format!(
        "{:<10} {:>8} {:>8} {:>6} {:>10} {:>8} {:>8} {:>11} {:>11}",
        "scenario", "size", "ms", "growth", "statements", "changes", "lookups", "scanned", "copied"
    )
}

fn line(name: &str, n: u64, m: &Measure, growth: Option<f64>) -> String {
    let growth = growth.map_or("-".into(), |g| format!("x{g:.1}"));
    let w = &m.work;
    format!(
        "{name:<10} {n:>8} {:>8.1} {growth:>6} {:>10} {:>8} {:>8} {:>11} {:>11}",
        m.ms, m.statements, w.changes, w.lookups, w.scanned, w.copied
    )
}

/// A results file: each line's measure, by scenario and size.
fn parse(text: &str) -> Result<BTreeMap<(String, u64), Measure>, String> {
    let mut out = BTreeMap::new();
    for (i, l) in text.lines().enumerate() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.is_empty() || f[0].starts_with('#') || l.starts_with("scenario ") {
            continue;
        }
        let bad = || format!("line {}: {l:?}", i + 1);
        let [name, n, ms, _, st, ch, lk, sc, cp] = f[..] else {
            return Err(bad());
        };
        let num = |s: &str| s.parse::<u64>().map_err(|_| bad());
        let work = Work {
            changes: num(ch)?,
            lookups: num(lk)?,
            scanned: num(sc)?,
            copied: num(cp)?,
        };
        let m = Measure {
            ms: ms.parse().map_err(|_| bad())?,
            statements: num(st)?,
            work,
        };
        out.insert((name.to_string(), num(n)?), m);
    }
    Ok(out)
}

/// What changed in the work, said briefly; empty when nothing did.
fn work_delta(was: &Measure, is: &Measure) -> String {
    let pairs = [
        ("statements", was.statements, is.statements),
        ("changes", was.work.changes, is.work.changes),
        ("lookups", was.work.lookups, is.work.lookups),
        ("scanned", was.work.scanned, is.work.scanned),
        ("copied", was.work.copied, is.work.copied),
    ];
    (pairs.iter())
        .filter(|(_, a, b)| a != b)
        .map(|(k, a, b)| format!("{k} {a} -> {b}"))
        .collect::<Vec<_>>()
        .join(", ")
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
    let file = root().join(format!("{host}.txt"));
    let recorded = match std::fs::read_to_string(&file) {
        Ok(text) if !record => match parse(&text) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                return ExitCode::FAILURE;
            }
        },
        _ => None,
    };
    let mut table = format!(
        "# Benchmarks on {host}: the median of {RUNS} runs, in milliseconds; growth\n\
         # is a size's time over the one before (x2 linear, x4 quadratic).\n\
         {TARGET}\n\n{}\n",
        header()
    );
    print!("{table}");
    let mut failures = Vec::new();
    // A time off, or a size not recorded: what recording again settles.
    let mut stale = false;
    for s in SCENARIOS {
        if !only.is_empty() && !only.iter().any(|o| *o == s.name) {
            continue;
        }
        let path = root().join("scenarios").join(format!("{}.adt", s.name));
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
            let l = line(s.name, n, &m, before.map(|b| m.ms / b));
            println!("{l}");
            table.push_str(&l);
            table.push('\n');
            before = Some(m.ms);
            let Some(recorded) = &recorded else { continue };
            let Some(was) = recorded.get(&(s.name.to_string(), n)) else {
                failures.push(format!("{} {n}: not in bench/{host}.txt", s.name));
                stale = true;
                continue;
            };
            let delta = work_delta(was, &m);
            let ratio = m.ms / was.ms;
            if was.ms >= FLOOR_MS && (ratio - 1.0).abs() > MARGIN {
                stale = true;
                let way = if ratio > 1.0 { "slower" } else { "faster" };
                let why = if delta.is_empty() {
                    "the same work".to_string()
                } else {
                    delta
                };
                failures.push(format!(
                    "{} {n}: {:.1} ms, was {:.1}: {:.0}% {way}; {why}",
                    s.name,
                    m.ms,
                    was.ms,
                    (ratio - 1.0).abs() * 100.0
                ));
            } else if !delta.is_empty() {
                println!("  work changed: {delta}");
            }
        }
    }
    if record {
        if let Err(e) = std::fs::write(&file, &table) {
            failures.push(format!("{}: {e}", file.display()));
        } else {
            println!("\nrecorded in bench/{host}.txt");
        }
    } else if recorded.is_none() {
        println!("\nno bench/{host}.txt: printed, not compared");
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
            "a change on purpose: `just bench-record` on {host}, and commit bench/{host}.txt"
        );
    }
    ExitCode::FAILURE
}
