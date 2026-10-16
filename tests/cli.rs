//! The command's `run`: the VM by default, the interpreter with
//! `--reference`, either on a store that lasts in a directory. That the
//! two engines agree is the gate's (`tests/vm.rs`); this is the wiring.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What `embryo-systems args` prints, failing the test if it fails.
fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_embryo-systems"))
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{args:?}: {stderr}");
    String::from_utf8(out.stdout).unwrap()
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
