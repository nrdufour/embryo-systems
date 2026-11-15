//! A-code no compiler wrote (docs/architecture.md, decisions 1 and 10):
//! the corpus's units, mutated a few thousand times. Each mutant is
//! refused when it is read, or runs to an end or to an error. None
//! panics: `exec` and the daemon take A-code from files and clients.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use adt81::compile::{Adt81, compile};
use adt81::lower;
use hbds_acode::Unit;
use hbds_gtad::Store;

#[derive(Default)]
struct Capture {
    input: VecDeque<String>,
}

impl hbds_vm::Io for Capture {
    fn read_line(&mut self) -> Option<String> {
        self.input.pop_front()
    }

    fn write(&mut self, _: &str) {}
}

/// Xorshift: the same mutants at each run.
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

fn programs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/adt81/tests/corpus/run");
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
    paths
}

/// One of the unit's instructions gone, twice, moved, or another's; or
/// one of the numbers its text holds, another number.
fn mutate(unit: &Unit, r: &mut Random) -> String {
    let mut unit = unit.clone();
    let m = r.below(unit.modules.len());
    let n = unit.modules[m].code.len();
    let at = r.below(n);
    match r.below(6) {
        0 => {
            unit.modules[m].code.remove(at);
            unit.modules[m].spans.remove(at);
        }
        1 => {
            let i = unit.modules[m].code[at].clone();
            let span = unit.modules[m].spans[at];
            unit.modules[m].code.insert(at, i);
            unit.modules[m].spans.insert(at, span);
        }
        2 => unit.modules[m].code.swap(at, r.below(n)),
        3 => {
            let from = r.below(unit.modules.len());
            let i = r.below(unit.modules[from].code.len());
            unit.modules[m].code[at] = unit.modules[from].code[i].clone();
        }
        _ => {
            // A number of an instruction's line, after its index and
            // its place: a slot, a target, a module, a count.
            let text = unit.text();
            let mut lines: Vec<String> = text.lines().map(String::from).collect();
            let numbered: Vec<(usize, Vec<usize>)> = (lines.iter().enumerate())
                .filter(|(_, l)| l.starts_with(' '))
                .map(|(n, l)| {
                    let words: Vec<&str> = l.split(' ').collect();
                    let numbers = (words.iter().enumerate())
                        .filter(|(_, w)| !w.is_empty())
                        .skip(2)
                        .filter(|(_, w)| w.parse::<u64>().is_ok())
                        .map(|(k, _)| k)
                        .collect();
                    (n, numbers)
                })
                .filter(|(_, numbers): &(usize, Vec<usize>)| !numbers.is_empty())
                .collect();
            if numbered.is_empty() {
                return text;
            }
            let (line, numbers) = &numbered[r.below(numbered.len())];
            let word = numbers[r.below(numbers.len())];
            let new = match r.below(4) {
                0 => 9999,
                _ => r.below(14),
            };
            let mut words: Vec<String> = lines[*line].split(' ').map(String::from).collect();
            words[word] = new.to_string();
            lines[*line] = words.join(" ");
            return lines.join("\n") + "\n";
        }
    }
    unit.text()
}

/// How a mutant ends.
enum End {
    Refused,
    Ran,
    Failed,
}

fn run(text: &str, input: &str) -> Result<End, String> {
    let Ok(unit) = Unit::read(text) else {
        return Ok(End::Refused);
    };
    let input = input.lines().map(String::from).collect();
    let ran = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let mut io = Capture { input };
            let mut store = Store::memory();
            let mut session = hbds_vm::Session::new(unit, Some(100_000));
            session.fuel(300_000);
            let mut sim = hbds_vm::Sequencing::new();
            loop {
                match session.turn(&mut store, &mut sim, &mut io, &Adt81, None) {
                    hbds_vm::Turn::Done(_) => return End::Ran,
                    hbds_vm::Turn::Failed(_) => return End::Failed,
                    _ => {}
                }
            }
        })
        .unwrap()
        .join();
    ran.map_err(|panic| {
        let why = (panic.downcast_ref::<String>().cloned())
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()));
        why.unwrap_or_default()
    })
}

#[test]
fn the_corpus_verifies() {
    for path in programs() {
        let source = fs::read_to_string(&path).unwrap();
        let unit = compile(&lower::parse_program(&source).unwrap());
        // What the compiler writes is read back: the verifier lets it.
        assert_eq!(
            Unit::read(&unit.text()).as_ref(),
            Ok(&unit),
            "{}",
            path.display()
        );
    }
}

#[test]
fn no_mutant_panics() {
    // The panics, if any, are told at the end, with their mutants.
    std::panic::set_hook(Box::new(|_| {}));
    let mut r = Random(0x9E37_79B9_7F4A_7C15);
    let (mut refused, mut ran, mut failed) = (0, 0, 0);
    let mut panics = Vec::new();
    for path in programs() {
        let source = fs::read_to_string(&path).unwrap();
        let input = fs::read_to_string(path.with_extension("in")).unwrap_or_default();
        let unit = compile(&lower::parse_program(&source).unwrap());
        for _ in 0..40 {
            let text = mutate(&unit, &mut r);
            match run(&text, &input) {
                Ok(End::Refused) => refused += 1,
                Ok(End::Ran) => ran += 1,
                Ok(End::Failed) => failed += 1,
                Err(why) => panics.push(format!("{}: {why}\n{text}", path.display())),
            }
        }
    }
    let _ = std::panic::take_hook();
    let mut reasons: Vec<&str> = (panics.iter())
        .map(|p| p.lines().next().unwrap_or_default())
        .map(|l| l.split_once(": ").map_or(l, |(_, why)| why))
        .collect();
    reasons.sort();
    reasons.dedup();
    assert!(
        panics.is_empty(),
        "{} mutant(s) panic, for {reasons:#?}; the first:\n{}",
        panics.len(),
        panics[0]
    );
    // Of each kind, enough for the test to mean something.
    assert!(
        refused > 1000 && ran > 100 && failed > 300,
        "{refused} refused, {ran} ran, {failed} failed"
    );
}
