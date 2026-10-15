//! The store across runs: a program builds a schema in a directory, a
//! second run, on a store opened afresh, reads it back; a journal cut short
//! by a crash loses only its last line; a corrupt one says where.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use adt81::interp::{self, Io};
use adt81::lower;
use hbds_gtad::Store;
use hbds_journal as journal;

#[derive(Default)]
struct Capture(String);

impl Io for Capture {
    fn read_line(&mut self) -> Option<String> {
        None
    }

    fn write(&mut self, text: &str) {
        self.0.push_str(text);
    }
}

/// A directory of its own for each test.
fn directory(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("embryo-systems-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// Runs `source` on `store`, its output.
fn run(source: &str, store: &Arc<Mutex<Store>>) -> String {
    let program = lower::parse_program(source).unwrap();
    let capture = Arc::new(Mutex::new(Capture::default()));
    interp::run(program, capture.clone(), store.clone()).unwrap();
    capture.lock().unwrap().0.clone()
}

fn open(dir: &Path) -> (Arc<Mutex<Store>>, Vec<String>) {
    let (store, warnings) = journal::open(dir).unwrap();
    (Arc::new(Mutex::new(store)), warnings)
}

const BUILD: &str = r#"
PROGRAM MAKER ;
BEGIN
        CREATE "TOWN", "RIVER" ;
        CREATE "TOWN" . "INHABITANTS", "TOWN" . "TEMP" ;
        SPECIFY "TOWN" . "TEMP" : FLOAT APPR UNIT (kelvin) ;
        LET "TOWN" . "TEMP" ' COND := "THIS . ""TEMP"" > 0." ;
        CREATE "RIVER" "crosses" "TOWN" REVERSE "crossed by" ;
        CREATE "Lyon" [ "TOWN" ], "Arles" [ "TOWN" ], "Rhone" [ "RIVER" ] ;
        LET "Lyon" [ "TOWN" ] . "INHABITANTS" := 522000 ;
        LET "Lyon" [ "TOWN" ] . "TEMP" := 284.5 +- 0.5 ;
        ASSERT "Rhone" [ "RIVER" ] "crosses" % 0.9 "Lyon" [ "TOWN" ] ;
        DESTROY "Arles" [ "TOWN" ]
END ;
"#;

const READ: &str = r#"
PROGRAM READER ;
LOCAL t : OBJECT ;
BEGIN
        t := "TOWN" ' OBJ ' FIRST ;
        PRINT "TOWN" ' OBJ, t . "INHABITANTS", t . "TEMP" ;
        PRINT t ' OPSCRC("crossed by", "RIVER") ' OFIN
END ;
"#;

const READ_OUT: &str = "[Lyon[TOWN]] 522000 284.5 +- 0.5\n[Rhone[RIVER]]\n";

#[test]
fn a_schema_outlives_its_run() {
    let dir = directory("outlives");
    let (store, warnings) = open(&dir);
    assert!(warnings.is_empty());
    run(BUILD, &store);
    let before = store.lock().unwrap().dump();
    drop(store);
    let (store, warnings) = open(&dir);
    assert!(warnings.is_empty());
    assert_eq!(store.lock().unwrap().dump(), before);
    assert_eq!(run(READ, &store), READ_OUT);
    assert!(
        before.contains(
            "attribute TEMP : FLOAT APPR UNIT (kelvin) COND (ADT'81) = \"THIS . \"TEMP\" > 0.\""
        ),
        "{before}"
    );
    assert!(before.contains("object Arles (destroyed)"), "{before}");
    assert!(
        before.contains("link Rhone[RIVER] \"crosses\" Lyon[TOWN] % 0.9"),
        "{before}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_torn_last_line_is_dropped() {
    let dir = directory("torn");
    let (store, _) = open(&dir);
    run(BUILD, &store);
    drop(store);
    let journal = dir.join("journal");
    let mut text = fs::read_to_string(&journal).unwrap();
    text.push_str("set 4 5 (f 1");
    fs::write(&journal, &text).unwrap();
    let (store, warnings) = open(&dir);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("cut short"));
    assert_eq!(run(READ, &store), READ_OUT);
    // What comes next is appended after the last whole line.
    run(
        r#"PROGRAM MORE ; BEGIN CREATE "Vienne" [ "TOWN" ] END ;"#,
        &store,
    );
    drop(store);
    let (store, warnings) = open(&dir);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(store.lock().unwrap().dump().contains("object Vienne"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_line_is_named() {
    let dir = directory("corrupt");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("journal"), "class 1 \"TOWN\" class\nclass 2\n").unwrap();
    let error = journal::open(&dir).err().expect("a corrupt journal").0;
    assert_eq!(error, "journal line 2: a term is missing");
    let _ = fs::remove_dir_all(&dir);
}

/// Runs `source` on `store`; its runtime error.
fn fails(source: &str, store: &Arc<Mutex<Store>>) -> String {
    let program = lower::parse_program(source).unwrap();
    let capture = Arc::new(Mutex::new(Capture::default()));
    interp::run(program, capture, store.clone())
        .expect_err("a runtime error")
        .to_string()
}

#[test]
fn a_refused_change_never_reaches_the_journal() {
    let dir = directory("refused");
    let (store, _) = open(&dir);
    let error = fails(
        r#"PROGRAM P ;
BEGIN
        CREATE "AREA" ; CREATE "AREA" . "temp" ; CREATE "a1" [ "AREA" ] ;
        LET "AREA" . "temp" ' COND := "THIS . ""temp"" > 0." ;
        LET "a1" [ "AREA" ] . "temp" := 10. ;
        LET "a1" [ "AREA" ] . "temp" := -5.
END ;"#,
        &store,
    );
    assert!(error.contains("refuses this change"), "{error}");
    assert!(store.lock().unwrap().dump().contains("temp = 10.0"));
    drop(store);
    let (store, _) = open(&dir);
    let dump = store.lock().unwrap().dump();
    assert!(dump.contains("temp = 10.0"), "{dump}");
    assert!(!dump.contains("-5"), "{dump}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn code_in_another_language_is_not_run() {
    let dir = directory("language");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("journal"),
        "class 1 \"AREA\" class\nattribute 2 1 \"temp\"\nproperty (ca 2) \"COND\" \"PROLOG\" \"temp(X) :- X > 0.\"\nobject 3 1 \"a1\"\n",
    )
    .unwrap();
    let (store, _) = open(&dir);
    let error = fails(
        r#"PROGRAM P ; BEGIN LET "a1" [ "AREA" ] . "temp" := 1. END ;"#,
        &store,
    );
    assert_eq!(
        error,
        "1:19: runtime error: the COND of AREA.temp is written in PROLOG, which ADT'81 does not run"
    );
    let _ = fs::remove_dir_all(&dir);
}
