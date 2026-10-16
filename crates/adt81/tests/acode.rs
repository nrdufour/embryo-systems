//! The compiler's output, golden: each program of `corpus/acode` beside
//! the A-code text it compiles to, written by hand before the compiler
//! ran. The text reads back to the same unit.

use std::fs;
use std::path::Path;

use adt81::{compile, lower};
use hbds_acode::Unit;

#[test]
fn compiled_as_written() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/acode");
    let mut seen = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "adt") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let program = lower::parse_program(&source).unwrap();
        let unit = compile::compile(&program);
        let text = unit.text();
        let expected = fs::read_to_string(path.with_extension("acode")).unwrap();
        assert_eq!(text, expected, "{}", path.display());
        assert_eq!(Unit::read(&text), Ok(unit));
        seen += 1;
    }
    assert!(seen >= 2);
}
