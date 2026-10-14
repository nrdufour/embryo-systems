//! Every program of the grammar's test corpus lexes.

use std::fs;
use std::path::Path;

use embryo_systems::adt81::lexer;

#[test]
fn corpus_lexes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/adt81");
    let mut count = 0;
    for dir in ["programs", "fragments"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            let source = fs::read_to_string(&path).unwrap();
            if let Err(e) = lexer::tokenize(&source) {
                panic!("{}:{e}", path.display());
            }
            count += 1;
        }
    }
    assert!(count >= 4, "corpus has {count} files");
}
