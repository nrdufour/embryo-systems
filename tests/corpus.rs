//! The grammar's test corpus: every program lexes and parses, every reject
//! is refused. `just check-grammar` also compares the trees with Lark's.

use std::fs;
use std::path::{Path, PathBuf};

use embryo_systems::adt81::{lexer, parser};

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/adt81");
    let files: Vec<_> = fs::read_dir(root.join(dir))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(!files.is_empty(), "no files in tests/adt81/{dir}");
    files
}

#[test]
fn corpus_lexes() {
    for path in corpus("programs").into_iter().chain(corpus("fragments")) {
        let source = fs::read_to_string(&path).unwrap();
        if let Err(e) = lexer::tokenize(&source) {
            panic!("{}:{e}", path.display());
        }
    }
}

#[test]
fn corpus_parses() {
    for (dir, program) in [("programs", true), ("fragments", false)] {
        for path in corpus(dir) {
            let source = fs::read_to_string(&path).unwrap();
            let tree = if program {
                parser::parse_program(&source)
            } else {
                parser::parse_fragment(&source)
            };
            if let Err(e) = tree {
                panic!("{}:{e}", path.display());
            }
        }
    }
}

#[test]
fn rejects_are_refused() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/adt81/reject.txt");
    let rejects = fs::read_to_string(path).unwrap();
    let lines = rejects
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
    for line in lines {
        assert!(parser::parse_fragment(line).is_err(), "accepted {line:?}");
    }
}
