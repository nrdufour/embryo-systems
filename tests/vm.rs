//! The gate (docs/architecture.md, decision 2): the VM gives exactly the
//! reference's outcome, its output, then how it ends (normally, by a
//! runtime error, its message and place, or past the statement budget),
//! and the statements it executed. Every corpus program the compiler
//! takes is run by both; those it leaves to a later slice are counted.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use adt81::compile::{Adt81, compile};
use adt81::diagnostic::Severity;
use adt81::{interp, lower};
use hbds_gtad::Store;

/// Generous: the corpus ends long before.
const BUDGET: u64 = 1_000_000;

#[derive(Default)]
struct Capture {
    input: VecDeque<String>,
    output: String,
}

impl interp::Io for Capture {
    fn read_line(&mut self) -> Option<String> {
        self.input.pop_front()
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
    }
}

impl hbds_vm::Io for Capture {
    fn read_line(&mut self) -> Option<String> {
        self.input.pop_front()
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
    }
}

fn capture(input: &str) -> Capture {
    Capture {
        input: input.lines().map(String::from).collect(),
        output: String::new(),
    }
}

/// What a run gives: its output, its error if it had one, and the
/// statements it executed if it ended well.
type Outcome = (String, Result<u64, String>);

fn reference(program: &adt81::ast::Program, input: &str) -> Outcome {
    let io = Arc::new(Mutex::new(capture(input)));
    let store = Arc::new(Mutex::new(Store::memory()));
    let r = interp::run_within(program.clone(), io.clone(), store, Some(BUDGET));
    let output = std::mem::take(&mut io.lock().unwrap().output);
    (output, r.map_err(|e| e.to_string()))
}

fn vm(unit: &hbds_acode::Unit, input: &str) -> Outcome {
    let mut io = capture(input);
    let r = hbds_vm::run(unit, &mut io, &Adt81, Some(BUDGET));
    (io.output, r.map_err(|e| e.to_string()))
}

fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "adt"))
        .collect();
    paths.sort();
    paths
}

/// Compiles `source` and runs it both ways: None if the compiler leaves
/// it to a later slice, else the two outcomes.
fn both(source: &str, input: &str) -> Option<(Outcome, Outcome, String)> {
    let program = lower::parse_program(source).unwrap_or_else(|e| panic!("{e}"));
    let errors = adt81::check(&program)
        .iter()
        .any(|d| d.severity == Severity::Error);
    assert!(!errors, "the program does not check");
    let unit = compile(&program).ok()?;
    Some((reference(&program, input), vm(&unit, input), unit.text()))
}

#[test]
fn the_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/adt81/tests/corpus/run");
    let mut paths = programs(&root);
    for dir in ["errors", "store", "store/errors"] {
        paths.extend(programs(&root.join(dir)));
    }
    let (mut run, mut skipped, mut differ) = (0, Vec::new(), Vec::new());
    for path in &paths {
        let source = fs::read_to_string(path).unwrap();
        let input = fs::read_to_string(path.with_extension("in")).unwrap_or_default();
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        match both(&source, &input) {
            None => skipped.push(name),
            Some((r, v, text)) => {
                run += 1;
                if r != v {
                    differ.push(format!(
                        "{name}\n  reference: {r:?}\n  vm:        {v:?}\n{text}"
                    ));
                }
            }
        }
    }
    println!(
        "vm gate: {run} programs run both ways, {} left to later slices",
        skipped.len()
    );
    assert!(differ.is_empty(), "\n{}", differ.join("\n"));
    assert!(run > 0);
}
