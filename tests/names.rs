//! Name resolution: one rule at a time, on programs written for it.

use std::fs;
use std::path::Path;

use embryo_systems::adt81::{lower, names, sample};

fn check(source: &str) -> Vec<String> {
    let program = lower::parse_program(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let diagnostics = names::resolve(&program).diagnostics;
    diagnostics.iter().map(ToString::to_string).collect()
}

fn assert_check(source: &str, expected: &[&str]) {
    assert_eq!(check(source), expected, "\n{source}");
}

#[test]
fn declared_names_pass() {
    assert_check("PROGRAM P ; LOCAL x : INTEGER ; BEGIN x := 1 END ;", &[]);
    // Names fold: total and TOTAL are one.
    assert_check(
        "PROGRAM P ; LOCAL total : FLOAT ; BEGIN TOTAL := 1. END ;",
        &[],
    );
}

#[test]
fn undeclared_names() {
    // Reported once per module, at the first use.
    assert_check(
        "PROGRAM P ; BEGIN y := 1 ; y := y + 1 END ;\nSUBROUTINE S ; BEGIN y := 1 END ;",
        &[
            "1:19: error: undeclared name `y`",
            "2:22: error: undeclared name `y`",
        ],
    );
    // Bounds, and every bare name in a designation.
    assert_check(
        "PROGRAM P ; LOCAL v : VECTOR [1:n] OF FLOAT ; BEGIN END ;",
        &["1:33: error: undeclared name `n`"],
    );
    assert_check(
        "PROGRAM P ;\nBEGIN\n    CREATE c, \"A\" [ k ] ;\n    x := o . \"X\"\nEND ;",
        &[
            "3:12: error: undeclared name `c`",
            "3:21: error: undeclared name `k`",
            "4:5: error: undeclared name `x`",
            "4:10: error: undeclared name `o`",
        ],
    );
}

#[test]
fn modules_defined_twice() {
    // Sorted by position, though found first.
    assert_check(
        "PROGRAM P ; BEGIN y := 1 END ;\nPROGRAM P ; BEGIN END ;",
        &[
            "1:19: error: undeclared name `y`",
            "2:9: error: module `P` is defined twice",
        ],
    );
    // A computed attribute's function has no name to clash.
    assert_check(
        "FUNCTION \"TOWN\" . \"DENSITY\" RETURNS FLOAT ;\nBEGIN RESULT THIS . \"INHABITANTS\" END ;\nFUNCTION \"ROAD\" . \"LENGTH\" RETURNS FLOAT ; BEGIN END ;",
        &[],
    );
}

#[test]
fn parameters() {
    assert_check(
        "SUBROUTINE S (a, a, b, c) ;\n    a : INTEGER IN ;\n    a, d : FLOAT IN ;\nBEGIN END ;",
        &[
            "1:18: error: parameter `a` is listed twice",
            "1:21: error: parameter `b` has no specification",
            "1:24: error: parameter `c` has no specification",
            "3:5: error: parameter `a` is specified twice",
            "3:8: error: `d` is specified but is not a parameter",
        ],
    );
}

#[test]
fn locals() {
    assert_check(
        "SUBROUTINE S (a) ;\n    a : INTEGER IN ;\nLOCAL\n    x, x : FLOAT ;\n    a : FLOAT ;\nBEGIN END ;",
        &[
            "4:8: error: `x` is declared twice",
            "5:5: error: `a` is already a parameter",
        ],
    );
}

#[test]
fn externals() {
    assert_check(
        "PROGRAM P ;\nLOCAL\n    F : FLOAT ;\nEXTERNAL\n    F : FUNCTION RETURNS FLOAT ;\n    S : FUNCTION RETURNS FLOAT ;\n    K : COROUTINE ;\nBEGIN RESUME K END ;\nSUBROUTINE S ; BEGIN END ;\nCOROUTINE K ; BEGIN END ;",
        &[
            "5:5: error: `F` is declared twice",
            "6:5: error: `S` is a subroutine in this file, not a function",
        ],
    );
}

#[test]
fn modules_are_not_variables() {
    assert_check(
        "PROGRAM P ; LOCAL x : INTEGER ; BEGIN x := S END ;\nSUBROUTINE S ; BEGIN END ;",
        &["1:44: error: `S` is a subroutine, not a variable"],
    );
}

#[test]
fn functors() {
    // Page 76: the prefix is declared, the functor's name is its result.
    assert_check(
        "FUNCTOR C ' F (D) RETURNS LIST OF CLASS ;\n    D : CLASS IN ;\nLOCAL\n    C : CLASS ;\nBEGIN F := C ' PSC INTER D ' NSC END ;",
        &[],
    );
    assert_check(
        "FUNCTOR C ' F RETURNS LIST OF CLASS ;\nBEGIN F := C ' PSC END ;",
        &[
            "1:9: error: the functor's prefix `C` is not declared",
            "2:12: error: undeclared name `C`",
        ],
    );
    assert_check(
        "PROGRAM P ; LOCAL L, M : LIST OF OBJECT ;\nBEGIN\n    L := M ' FIRST ' NOWHERE ' MINE ;\n    L := M ' L\nEND ;\nFUNCTOR MINE RETURNS LIST OF OBJECT ; BEGIN END ;",
        &[
            "3:22: warning: unknown functor `NOWHERE`",
            "4:14: error: `L` is a variable, not a functor",
        ],
    );
}

#[test]
fn calls() {
    assert_check(
        "PROGRAM P ; LOCAL x : INTEGER ;\nBEGIN\n    CALL F ;\n    RESUME S ;\n    CHAIN Q ;\n    CALL NOWHERE ;\n    x := SQRT(x) + x(1) ;\n    CHAIN P\nEND ;\nFUNCTION F RETURNS FLOAT ; BEGIN END ;\nSUBROUTINE S ; BEGIN END ;",
        &[
            "3:10: error: `F` is a function, not a subroutine",
            "4:12: error: `S` is a subroutine, not a coroutine",
            "5:11: error: unknown program `Q`",
            "6:10: error: unknown subroutine `NOWHERE`",
            "7:10: warning: unknown function `SQRT`",
            "7:20: error: `x` is a variable, not a function",
        ],
    );
}

#[test]
fn properties() {
    assert_check(
        "PROGRAM P ;\nBEGIN\n    LET \"A\" . \"x\" ' COND := \"x > 0\" ;\n    LET \"A\" . \"x\" ' WHEN_READ := \"x\"\nEND ;",
        &["4:21: warning: unknown property `WHEN_READ`"],
    );
}

#[test]
fn units_are_not_checked() {
    assert_check(
        "PROGRAM P ;\nBEGIN SPECIFY \"A\" . \"t\" : FLOAT UNIT (celsius) END ;",
        &[],
    );
}

#[test]
fn clean_programs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/adt81");
    for entry in fs::read_dir(root.join("checked")).unwrap() {
        let path = entry.unwrap().path();
        let source = fs::read_to_string(&path).unwrap();
        assert_eq!(check(&source), Vec::<String>::new(), "{}", path.display());
    }
    let statistics = fs::read_to_string(root.join("programs/statistics.adt")).unwrap();
    assert_eq!(
        check(&statistics),
        ["20:15: warning: unknown function `SQRT`"]
    );
}

#[test]
fn random_programs_resolve() {
    for seed in 0..2000 {
        // Printed and read back, for real positions.
        let program = lower::parse_program(&sample::program(seed).to_string()).unwrap();
        let diagnostics = names::resolve(&program).diagnostics;
        let positions: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.span.line, d.span.column))
            .collect();
        assert!(positions.is_sorted(), "seed {seed}");
    }
}
