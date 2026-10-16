//! Kinds and types: one rule at a time, on programs written for it.

use std::fs;
use std::path::Path;

use adt81::diagnostic::Diagnostic;
use adt81::{lower, names, sample, typeck};

/// Both layers, as `embryo-systems check` runs them.
fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let program = lower::parse_program(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let resolution = names::resolve(&program);
    let mut all = typeck::check_types(&program, &resolution);
    all.extend(resolution.diagnostics);
    all.sort_by_key(|d| (d.span.line, d.span.column));
    all
}

fn check(source: &str) -> Vec<String> {
    diagnostics(source)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The diagnostics of `statements` in a program declaring `locals`, their
/// lines counted from the first statement's.
fn body(locals: &str, statements: &str) -> Vec<String> {
    let source = format!("PROGRAM P ;\nLOCAL\n{locals}\nBEGIN\n{statements}\nEND ;");
    let offset = 3 + locals.lines().count();
    diagnostics(&source)
        .iter()
        .map(|d| {
            let mut d = d.clone();
            d.span.line -= offset;
            d.to_string()
        })
        .collect()
}

const LOCALS: &str = "n : INTEGER ; x : FLOAT ; y : FLOAT APPR ; r : RATIONAL ;
z : COMPLEX ; g : GAUSS ; b : BOOLEAN ; c : CHARACTER ; s : STRING ;
v : VECTOR OF FLOAT ; L : LIST OF OBJECT ; o : OBJECT ; k : CLASS ;";

fn assert_body(statements: &str, expected: &[&str]) {
    assert_eq!(body(LOCALS, statements), expected, "\n{statements}");
}

/// EXIT, NEXT and RELOOP count the loops around them in their module: a
/// count written out is checked, a computed one is left to the run.
#[test]
fn loops_counted_lexically() {
    assert_body(
        "exit",
        &["1:1: error: EXIT 1 counts 1 loop(s), and no loop is around it in its module"],
    );
    assert_body(
        "loop do next 2 endloop",
        &["1:9: error: NEXT 2 counts 2 loop(s), and one loop is around it in its module"],
    );
    assert_body("loop do loop do reloop 2 endloop endloop", &[]);
    assert_body("loop do exit n endloop", &[]);
}

#[test]
fn logic_and_comparison() {
    assert_body(
        "b := b AND n",
        &["1:12: error: `AND` takes booleans, not an integer"],
    );
    assert_body(
        "b := NOT n",
        &["1:10: error: NOT's operand is an integer, not a boolean"],
    );
    assert_body("b := n < x AND s >= \"a\" OR b", &[]);
    assert_body("b := z = z", &[]);
    assert_body(
        "b := z < z",
        &["1:6: error: `<` does not order a complex number: only = and /= compare these (page 27)"],
    );
    assert_body(
        "b := g <= 2",
        &["1:6: error: `<=` does not order a Gauss integer: only = and /= compare these (page 27)"],
    );
    assert_body(
        "b := n = \"x\"",
        &["1:6: error: `=` cannot compare an integer with a character"],
    );
    assert_body("b := o IN L", &[]);
    assert_body(
        "b := n IN n",
        &["1:11: error: `IN` looks in a list, not in an integer"],
    );
}

#[test]
fn lists_and_strings() {
    assert_body("L := L UNION L INTER L COMPL L", &[]);
    assert_body(
        "L := L UNION n",
        &["1:14: error: `UNION` takes lists, not an integer"],
    );
    assert_body("s := s & c & \"x\"", &[]);
    assert_body(
        "s := s & n",
        &["1:10: error: `&` takes characters and strings, not an integer"],
    );
}

#[test]
fn arithmetic() {
    assert_body(
        "x := n + b",
        &["1:10: error: `+` takes numbers, not a boolean"],
    );
    assert_body("n := 7 / 2", &[]);
    assert_body(
        "n := 7.0 / 2",
        &["1:6: error: cannot assign a float to `n`, an integer"],
    );
    assert_body(
        "n := 7 MOD 2.0",
        &["1:12: error: `MOD` takes integers, not a float"],
    );
    assert_body("r := 3 // 4", &[]);
    assert_body(
        "n := 3 // 4",
        &["1:6: error: cannot assign a rational to `n`, an integer"],
    );
    assert_body(
        "x := 2.0 ^ z",
        &["1:12: error: an exponent is an integer, a rational or a float, not a complex number"],
    );
    assert_body("x := -n ^ 2", &[]);
    assert_body(
        "b := -b",
        &["1:7: error: a sign applies to a number, not a boolean"],
    );
}

#[test]
fn widening() {
    assert_body("z := n + x ; g := n + 2i ; r := n ; x := r", &[]);
    assert_body("z := g * 2.0", &[]);
    assert_body(
        "g := x",
        &["1:6: error: cannot assign a float to `g`, a Gauss integer"],
    );
    assert_body(
        "x := z",
        &["1:6: error: cannot assign a complex number to `x`, a float"],
    );
    assert_body("c := \"a\" ; s := c", &[]);
    assert_body(
        "c := \"ab\"",
        &["1:6: error: cannot assign a string to `c`, a character"],
    );
}

#[test]
fn fuzziness() {
    assert_body("y := 1.0 +- 0.1 ; y := y * 2.0", &[]);
    assert_body(
        "x := 1.0 +- 0.1",
        &["1:6: error: `x` is not declared APPR, yet receives such a value (page 152)"],
    );
    assert_body(
        "y := 1.0 % 0.5 @\"so they say\"",
        &["1:6: error: `y` is not declared PROB SUBJ, yet receives such a value (page 152)"],
    );
    assert_body(
        "x := { 1.0, 2.0 }",
        &["1:6: error: `x` is not declared MULT, yet receives such a value (page 152)"],
    );
    assert_body(
        "x := y * 2.0",
        &["1:6: error: `x` is not declared APPR, yet receives such a value (page 152)"],
    );
}

#[test]
fn indices() {
    assert_body("x := v[n] ; v[1] := x", &[]);
    assert_body(
        "x := v[b]",
        &["1:8: error: an index is a boolean, not an integer"],
    );
    assert_body(
        "x := v[1, 2]",
        &["1:6: error: `v` has 2 index(es): a vector takes 1 index"],
    );
    assert_body(
        "x := n[1]",
        &["1:6: error: `n` is an integer, not an array"],
    );
}

#[test]
fn statements() {
    assert_body(
        "IF n THEN ENDIF",
        &["1:4: error: the condition is an integer, not a boolean"],
    );
    assert_body(
        "LOOP WHILE x DO ENDLOOP",
        &["1:12: error: the condition is a float, not a boolean"],
    );
    assert_body(
        "LOOP FOR b : 1 TO 2 DO ENDLOOP",
        &["1:10: error: the loop variable `b` is a boolean, not a number"],
    );
    assert_body(
        "LOOP FOR n : 1 TO 2.5 DO ENDLOOP",
        &["1:19: error: cannot assign a float to `n`, an integer"],
    );
    assert_body("EACH o OF L DO ENDEACH", &[]);
    assert_body(
        "EACH n OF L DO ENDEACH",
        &["1:6: error: `n` is an integer, and goes over a list of object"],
    );
    assert_body(
        "EACH o OF n DO ENDEACH",
        &["1:11: error: EACH goes over a list, not an integer"],
    );
    assert_body(
        "EXIT 1.5",
        &["1:6: error: the number of loops is a float, not an integer"],
    );
    assert_body(
        "DELAY o FOR b",
        &["1:13: error: the duration is a boolean, not a number"],
    );
    assert_body(
        "CASE n WHEN \"a\" DO ENDCASE",
        &["1:13: error: `=` cannot compare an integer with a character"],
    );
    assert_body("EMPTY n", &["1:7: error: `n` is an integer, not a list"]);
    assert_body("INSERT o INTO L", &[]);
    assert_body("RESULT 1", &["1:1: error: RESULT is for functions only"]);
}

#[test]
fn parameters_and_calls() {
    let source = "PROGRAM P ;
LOCAL n : INTEGER ; x : FLOAT ;
BEGIN
    CALL S (n, x) ;
    CALL S (1.5, n) ;
    CALL S (n, 2.0) ;
    CALL S (n) ;
    x := F(n) + F(TRUE)
END ;
SUBROUTINE S (a, b) ;
    a : INTEGER IN ;
    b : FLOAT OUT ;
BEGIN
    a := 1 ;
    b := a
END ;
FUNCTION F (u) RETURNS INTEGER ;
    u : FLOAT IN ;
BEGIN
    RESULT u
END ;";
    assert_eq!(
        check(source),
        [
            "5:13: error: cannot assign a float to argument 1 of `S`, an integer",
            "5:18: error: cannot assign a float to argument 2 of `S`, an integer",
            "6:16: error: argument 2 of `S` is OUT: it must be a variable",
            "7:10: error: `S` takes 2 argument(s), not 1",
            "8:19: error: cannot assign a boolean to argument 1 of `F`, a float",
            "14:5: error: `a` is an IN parameter: it cannot be assigned",
            "20:12: error: cannot assign a float to the function's result, an integer",
        ]
    );
}

#[test]
fn adt_kinds() {
    assert_body(
        "CREATE \"A\", \"p\" [ \"A\" ]",
        &["1:13: error: CREATE mixes classes and objects"],
    );
    assert_body(
        "ASSOC \"A\" . \"x\" WITH o",
        &["1:7: error: ASSOC cannot pair a class attribute WITH an object"],
    );
    assert_body(
        "DISSOC o FROM o",
        &["1:8: error: DISSOC cannot pair an object FROM an object"],
    );
    assert_body(
        "CREATE n",
        &["1:8: error: `n` is an integer, not an a.d.t. or its name"],
    );
    assert_body(
        "ACTIVATE k",
        &["1:10: error: a class is used where an object is expected"],
    );
    assert_body(
        "x := k . \"X\"",
        &["1:6: error: a class is used where an object is expected"],
    );
    assert_body(
        "b := o \"near\" k",
        &["1:15: error: a class is used where an object is expected"],
    );
    assert_body(
        "INCLUDE o INTO k",
        &["1:9: error: an object is used where a class is expected"],
    );
    assert_body("CREATE s ; COMPOSE s WITH \"A\", k", &[]);
}

#[test]
fn functors() {
    assert_body(
        "L := k ' OBJ ; o := L ' FIRST ; n := L ' CARD ; L := L ' SUBLIST(1, 2)",
        &[],
    );
    assert_body(
        "L := o ' OBJ",
        &["1:10: error: `OBJ` applies to classes (page 76), not to an object"],
    );
    assert_body(
        "n := o ' CARD",
        &["1:10: error: `CARD` applies to a list (page 80), not to an object"],
    );
    assert_body(
        "n := L ' FIRST",
        &["1:6: error: cannot assign an object to `n`, an integer"],
    );
    assert_body(
        "L := o ' OPSCRC(\"near\", \"TOWN\") ' OFIN ' OBJOF ' OBJ",
        &[],
    );
    assert_body(
        "L := n ' OBJ",
        &["1:6: error: `n` is an integer, not an a.d.t. or its name"],
    );
    // List functors take lists of anything (page 80); a.d.t. statements do
    // not.
    assert_eq!(
        body(
            "K : LIST OF INTEGER ; i : INTEGER ;",
            "i := K ' CARD ; i := K ' FIRST ; ACTIVATE K"
        ),
        ["1:43: error: a list of integer is used where an object is expected"]
    );
    assert_eq!(
        check("FUNCTOR C ' F RETURNS LIST ;\nLOCAL C : INTEGER ;\nBEGIN END ;"),
        ["1:9: error: the functor's prefix `C` is an integer, not an a.d.t. or a list"]
    );
}

#[test]
fn unknown_values_pass() {
    // An object's attribute, an unknown functor's result: nothing to prove.
    assert_body(
        "n := o . \"X\" ; b := o . \"Y\" AND b ; x := L ' NOWHERE + 1",
        &["1:46: warning: unknown functor `NOWHERE`"],
    );
}

#[test]
fn clean_programs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    for entry in fs::read_dir(root.join("checked")).unwrap() {
        let path = entry.unwrap().path();
        let source = fs::read_to_string(&path).unwrap();
        assert_eq!(check(&source), Vec::<String>::new(), "{}", path.display());
    }
    let statistics = fs::read_to_string(root.join("programs/statistics.adt")).unwrap();
    // SQRT is one of our math functions (docs/adt81.md, "Running").
    assert_eq!(check(&statistics), Vec::<String>::new());
}

#[test]
fn random_programs_check() {
    for seed in 0..2000 {
        let text = sample::program(seed).to_string();
        let positions: Vec<_> = diagnostics(&text)
            .iter()
            .map(|d| (d.span.line, d.span.column))
            .collect();
        assert!(positions.is_sorted(), "seed {seed}");
    }
}
