//! Golden runs: each program in tests/corpus/run, written for the
//! purpose, checks clean, runs on its `.in`, and prints its `.out`, a
//! runtime error included as the last line.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use adt81::diagnostic::Severity;
use adt81::interp::{self, Io};
use adt81::lower;
use hbds_gtad::Store;

#[derive(Default)]
struct Capture {
    input: VecDeque<String>,
    output: String,
}

impl Io for Capture {
    fn read_line(&mut self) -> Option<String> {
        self.input.pop_front()
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
    }
}

/// The program's output, and its runtime error if any.
fn run(source: &str, input: &str) -> String {
    let program = lower::parse_program(source).unwrap_or_else(|e| panic!("{e}"));
    let errors: Vec<String> = adt81::check(&program)
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    assert!(errors.is_empty(), "the program does not check: {errors:?}");
    let capture = Arc::new(Mutex::new(Capture {
        input: input.lines().map(String::from).collect(),
        output: String::new(),
    }));
    let store = Arc::new(Mutex::new(Store::memory()));
    let result = interp::run(program, capture.clone(), store.clone());
    if let Err(e) = store.lock().unwrap().verify() {
        panic!("{e}");
    }
    let mut output = capture.lock().unwrap().output.clone();
    if let Err(e) = result {
        output += &format!("{e}\n");
    }
    output
}

fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "adt"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn golden_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/run");
    let mut paths = programs(&root);
    for dir in ["errors", "store", "store/errors"] {
        paths.extend(programs(&root.join(dir)));
    }
    assert!(paths.len() >= 10, "{} programs", paths.len());
    let mut failures = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path).unwrap();
        let input = fs::read_to_string(path.with_extension("in")).unwrap_or_default();
        let expected = fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|_| panic!("{} has no .out", path.display()));
        let output = run(&source, &input);
        if output != expected {
            failures.push(format!(
                "{}\n--- expected\n{expected}--- got\n{output}",
                path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The clean programs run to their end or to a clean runtime error, such
/// as an a.d.t. statement before the store exists; never a panic.
#[test]
fn checked_programs_run_cleanly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/checked");
    for path in programs(&root) {
        let source = fs::read_to_string(&path).unwrap();
        run(&source, "");
    }
}
