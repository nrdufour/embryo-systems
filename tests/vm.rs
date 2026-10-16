//! The gate (docs/architecture.md, decision 2): the VM gives exactly the
//! reference's outcome, its output, then how it ends (normally, by a
//! runtime error, its message and place, or past the statement budget),
//! and the statements it executed. Every corpus program is run by both.

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
/// statements it executed if it ended well; then what the store holds.
type Outcome = (String, Result<u64, String>, String);

fn reference(program: &adt81::ast::Program, input: &str, budget: u64) -> Outcome {
    let io = Arc::new(Mutex::new(capture(input)));
    let store = Arc::new(Mutex::new(Store::memory()));
    let r = interp::run_within(program.clone(), io.clone(), store.clone(), Some(budget));
    let output = std::mem::take(&mut io.lock().unwrap().output);
    let dump = store.lock().unwrap().dump();
    (output, r.map_err(|e| e.to_string()), dump)
}

fn vm(unit: &hbds_acode::Unit, input: &str, budget: u64) -> Outcome {
    let mut io = capture(input);
    let mut store = Store::memory();
    let r = hbds_vm::run(unit, &mut store, &mut io, &Adt81, Some(budget));
    (io.output, r.map_err(|e| e.to_string()), store.dump())
}

/// The VM again, in turns of a few instructions, as the daemon runs a
/// session: the same outcome.
fn vm_in_turns(unit: &hbds_acode::Unit, input: &str, budget: u64) -> Outcome {
    let mut io = capture(input);
    let mut store = Store::memory();
    let r = std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn_scoped(s, || {
                let mut session = hbds_vm::Session::new(unit.clone(), Some(budget));
                loop {
                    match session.turn(&mut store, &mut io, &Adt81, Some(5)) {
                        hbds_vm::Turn::Done(n) => return Ok(n),
                        hbds_vm::Turn::Failed(e) => return Err(e.to_string()),
                        hbds_vm::Turn::Yield | hbds_vm::Turn::Input => {}
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap()
    });
    (io.output, r, store.dump())
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

/// Compiles `source` and runs it both ways: the two outcomes, and the
/// A-code.
fn both(source: &str, input: &str) -> (Outcome, Outcome, String) {
    let program = lower::parse_program(source).unwrap_or_else(|e| panic!("{e}"));
    let errors = adt81::check(&program)
        .iter()
        .any(|d| d.severity == Severity::Error);
    assert!(!errors, "the program does not check");
    let unit = compile(&program);
    (
        reference(&program, input, BUDGET),
        vm(&unit, input, BUDGET),
        unit.text(),
    )
}

#[test]
fn the_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/adt81/tests/corpus/run");
    let mut paths = programs(&root);
    for dir in ["errors", "store", "store/errors"] {
        paths.extend(programs(&root.join(dir)));
    }
    paths.extend(programs(&root.join("../acode")));
    let mut differ = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(path).unwrap();
        let input = fs::read_to_string(path.with_extension("in")).unwrap_or_default();
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        let (r, v, text) = both(&source, &input);
        if r != v {
            differ.push(format!(
                "{name}\n  reference: {r:?}\n  vm:        {v:?}\n{text}"
            ));
        }
        let program = lower::parse_program(&source).unwrap();
        let t = vm_in_turns(&compile(&program), &input, BUDGET);
        if t != v {
            differ.push(format!(
                "{name}, in turns\n  vm:       {v:?}\n  in turns: {t:?}"
            ));
        }
    }
    println!("vm gate: {} programs run both ways", paths.len());
    assert!(differ.is_empty(), "\n{}", differ.join("\n"));
    assert!(!paths.is_empty());
}

/// Random programs: those that check run both ways, under a budget small
/// enough that one that never ends ends both runs alike.
#[test]
fn random_programs() {
    const SEEDS: u64 = 2000;
    const SMALL: u64 = 3000;
    let (mut checked, mut errors, mut spent, mut differ) = (0, 0, 0, Vec::new());
    let mut rejected = Vec::new();
    for seed in 0..SEEDS {
        let (source, input) = adt81::sample::runnable(seed);
        let program = lower::parse_program(&source)
            .unwrap_or_else(|e| panic!("seed {seed} does not parse: {e}\n{source}"));
        let diagnostics: Vec<String> = (adt81::check(&program).iter())
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.to_string())
            .collect();
        if !diagnostics.is_empty() {
            rejected.push(diagnostics[0].clone());
            continue;
        }
        checked += 1;
        let unit = compile(&program);
        let r = reference(&program, &input, SMALL);
        let v = vm(&unit, &input, SMALL);
        match &r.1 {
            Err(e) if e.ends_with(interp::BUDGET_SPENT) => spent += 1,
            Err(e) => {
                errors += 1;
                if std::env::var("VM_ERRORS").is_ok() {
                    println!("ERROR {}", e.split("runtime error: ").nth(1).unwrap_or(e));
                }
            }
            Ok(_) => {}
        }
        if r != v && differ.len() < 3 {
            differ.push(format!(
                "seed {seed}\n  reference: {r:?}\n  vm:        {v:?}\n{source}\ninput: {input:?}"
            ));
        }
    }
    rejected.sort();
    println!(
        "random: {checked} of {SEEDS} check; {errors} end in a runtime error, {spent} past the budget"
    );
    if let Some(r) = rejected.first() {
        println!("a rejection: {r}");
    }
    assert!(differ.is_empty(), "\n{}", differ.join("\n"));
    assert!(checked * 2 > SEEDS, "too few random programs check");
}
