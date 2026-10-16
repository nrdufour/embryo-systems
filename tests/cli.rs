//! The command's `run`: the VM by default, the interpreter with
//! `--reference`, either on a store that lasts in a directory; `exec` of
//! an `.acode` file, and the executables `build` writes. That the two
//! engines agree is the gate's (`tests/vm.rs`); this is the wiring.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_embryo-systems");

/// What `program args` prints given `input`, and its errors; whether it
/// succeeded.
fn launch(program: &str, args: &[&str], input: &str) -> (bool, String, String) {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = |b: Vec<u8>| String::from_utf8(b).unwrap();
    (out.status.success(), text(out.stdout), text(out.stderr))
}

/// What `program args` prints, failing the test if it fails.
fn ok(program: &str, args: &[&str], input: &str) -> String {
    let (success, out, err) = launch(program, args, input);
    assert!(success, "{program} {args:?}: {err}");
    out
}

/// What `embryo-systems args` prints, failing the test if it fails.
fn run(args: &[&str]) -> String {
    ok(BIN, args, "")
}

fn corpus(name: &str) -> String {
    format!(
        "{}/crates/adt81/tests/corpus/run/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// A directory of its own for `name`, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn both_engines_run_a_program() {
    let hello = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/crates/adt81/tests/corpus/run/hello.adt"
    );
    assert_eq!(run(&["run", hello]), "Hello, world!\n");
    assert_eq!(run(&["run", "--reference", hello]), "Hello, world!\n");
}

#[test]
fn a_store_lasts_between_runs_of_either_engine() {
    let dir = scratch("cli-store");
    let write = dir.join("write.adt");
    let read = dir.join("read.adt");
    fs::write(
        &write,
        "program keeps ;\nbegin\n        create \"c\" ; create \"c\" . \"a\" ;\n        create \"o\" [ \"c\" ] ;\n        let \"o\" [ \"c\" ] . \"a\" := 7\nend ;\n",
    )
    .unwrap();
    fs::write(
        &read,
        "program finds ;\nbegin\n        print \"o\" [ \"c\" ] . \"a\"\nend ;\n",
    )
    .unwrap();
    let (write, read) = (write.to_str().unwrap(), read.to_str().unwrap());
    for (first, second) in [(None, Some("--reference")), (Some("--reference"), None)] {
        let store = scratch(&format!("cli-store-{}", first.is_some()));
        let store = store.to_str().unwrap();
        let args = |engine: Option<&'static str>, file| {
            let mut a = vec!["run", "--store", store];
            a.extend(engine);
            a.push(file);
            a
        };
        assert_eq!(run(&args(first, write)), "");
        assert_eq!(run(&args(second, read)), "7\n");
    }
}

#[test]
fn exec_runs_what_acode_wrote() {
    let dir = scratch("cli-exec");
    let file = dir.join("io.acode");
    fs::write(&file, run(&["acode", &corpus("io.adt")])).unwrap();
    let input = fs::read_to_string(corpus("io.in")).unwrap();
    let expected = fs::read_to_string(corpus("io.out")).unwrap();
    assert_eq!(ok(BIN, &["exec", file.to_str().unwrap()], &input), expected);
}

#[test]
fn exec_refuses_what_it_cannot_run() {
    let dir = scratch("cli-refused");
    let text = run(&["acode", &corpus("hello.adt")]);
    for (name, text, message) in [
        (
            "newer.acode",
            text.replacen("acode 1", "acode 2", 1),
            "A-code version 2 is newer than this reader's, 1",
        ),
        (
            "other.acode",
            text.replacen("language \"ADT'81\"", "language \"PL/I\"", 1),
            "A-code in PL/I, a language this embryo-systems does not have",
        ),
    ] {
        let file = dir.join(name);
        fs::write(&file, text).unwrap();
        let (success, _, err) = launch(BIN, &["exec", file.to_str().unwrap()], "");
        assert!(!success && err.contains(message), "{name}: {err}");
    }
}

#[test]
fn a_built_program_runs_on_its_own() {
    let dir = scratch("cli-build");
    let program = dir.join("io");
    run(&["build", &corpus("io.adt"), "-o", program.to_str().unwrap()]);
    // Elsewhere, under another name: it carries all it needs.
    let elsewhere = scratch("cli-build-elsewhere").join("copied");
    fs::copy(&program, &elsewhere).unwrap();
    let input = fs::read_to_string(corpus("io.in")).unwrap();
    let expected = fs::read_to_string(corpus("io.out")).unwrap();
    assert_eq!(ok(elsewhere.to_str().unwrap(), &[], &input), expected);
    let (success, _, err) = launch(elsewhere.to_str().unwrap(), &["io.adt"], "");
    assert!(!success && err.contains("[--store DIR]"), "{err}");
}

#[test]
fn a_built_program_keeps_its_store() {
    let dir = scratch("cli-build-store");
    let keeps = dir.join("keeps.adt");
    fs::write(
        &keeps,
        "program keeps ;\nbegin\n        create \"c\" ; create \"c\" . \"a\" ;\n        create \"o\" [ \"c\" ] ;\n        let \"o\" [ \"c\" ] . \"a\" := 7\nend ;\n",
    )
    .unwrap();
    let finds = dir.join("finds.adt");
    fs::write(
        &finds,
        "program finds ;\nbegin\n        print \"o\" [ \"c\" ] . \"a\"\nend ;\n",
    )
    .unwrap();
    let program = dir.join("keeps");
    run(&[
        "build",
        keeps.to_str().unwrap(),
        "-o",
        program.to_str().unwrap(),
    ]);
    let store = dir.join("store");
    let store = store.to_str().unwrap();
    assert_eq!(ok(program.to_str().unwrap(), &["--store", store], ""), "");
    assert_eq!(
        run(&["run", "--store", store, finds.to_str().unwrap()]),
        "7\n"
    );
    // And as an .acode file, by exec.
    let acode = dir.join("keeps.acode");
    fs::write(&acode, run(&["acode", keeps.to_str().unwrap()])).unwrap();
    let other = dir.join("other");
    let other = other.to_str().unwrap();
    assert_eq!(
        run(&["exec", "--store", other, acode.to_str().unwrap()]),
        ""
    );
    assert_eq!(
        run(&["run", "--store", other, finds.to_str().unwrap()]),
        "7\n"
    );
}
