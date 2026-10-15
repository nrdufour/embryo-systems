use std::io::{BufRead, Read, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use adt81::diagnostic::Severity;
use adt81::{interp, lexer, lower, parser};
use hbds_gtad::Store;
use hbds_journal as journal;

const USAGE: &str = "usage: embryo-systems lex FILE
       embryo-systems parse [--program | --fragment] FILE
       embryo-systems fmt [--program | --fragment] FILE
       embryo-systems check FILE
       embryo-systems run [--store DIR] FILE
       embryo-systems store DIR

FILE - reads standard input, but for run. parse prints the syntax tree,
fmt the source in canonical form; both read a program unless told
--fragment. check reports undeclared and misused names, and type errors,
in a program, and fails on errors. run checks a program, then runs it:
READ reads standard input, PRINT writes standard output; its classes and
objects last the run, or go in the store in DIR, created if need be.
store prints what the store in DIR holds.";

/// READ on standard input, PRINT on standard output.
struct Terminal;

impl interp::Io for Terminal {
    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }

    fn write(&mut self, text: &str) {
        print!("{text}");
        let _ = std::io::stdout().flush();
    }
}

/// The store in `dir`, or one for the run; its warnings on stderr.
fn open_store(dir: Option<&str>) -> Result<Store, String> {
    let Some(dir) = dir else {
        return Ok(Store::memory());
    };
    let (store, warnings) = journal::open(std::path::Path::new(dir)).map_err(|e| e.0)?;
    for w in warnings {
        eprintln!("{dir}: {w}");
    }
    Ok(store)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["store", dir] = args.as_slice() {
        if !std::path::Path::new(dir).join("journal").exists() {
            eprintln!("{dir}: no store here");
            return ExitCode::FAILURE;
        }
        return match open_store(Some(dir)) {
            Ok(store) => {
                print!("{}", store.dump());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let (args, store_dir) = match args.as_slice() {
        ["run", "--store", dir, rest @ ..] => {
            let mut a = vec!["run"];
            a.extend(rest);
            (a, Some(*dir))
        }
        _ => (args.clone(), None),
    };
    let (command, fragment, path) = match args.as_slice() {
        [command @ ("lex" | "check"), path] => (*command, false, *path),
        ["run", path] if *path != "-" => ("run", false, *path),
        [command @ ("parse" | "fmt"), "--program", path] | [command @ ("parse" | "fmt"), path] => {
            (*command, false, *path)
        }
        [command @ ("parse" | "fmt"), "--fragment", path] => (*command, true, *path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    // The language, by the file's extension: ADT'81 is the only one yet.
    if path != "-" && !path.ends_with(".adt") {
        eprintln!("{path}: no language for this file; ADT'81 files end in .adt");
        return ExitCode::FAILURE;
    }
    let source = if path == "-" {
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source).map(|_| source)
    } else {
        std::fs::read_to_string(path)
    };
    let source = match source {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Whether the source passes: only `check` can fail on a source it reads.
    let result = if command == "lex" {
        lexer::tokenize(&source).map(|tokens| {
            for t in tokens {
                println!("{}:{}\t{:?}\t{}", t.line, t.column, t.kind, t.text);
            }
            true
        })
    } else if command == "check" {
        lower::parse_program(&source).map(|program| {
            let diagnostics = adt81::check(&program);
            for d in &diagnostics {
                println!("{path}:{d}");
            }
            diagnostics.iter().all(|d| d.severity != Severity::Error)
        })
    } else if command == "run" {
        lower::parse_program(&source).map(|program| {
            let diagnostics = adt81::check(&program);
            for d in &diagnostics {
                eprintln!("{path}:{d}");
            }
            if diagnostics.iter().any(|d| d.severity == Severity::Error) {
                return false;
            }
            let store = match open_store(store_dir) {
                Ok(store) => Arc::new(Mutex::new(store)),
                Err(e) => {
                    eprintln!("{e}");
                    return false;
                }
            };
            match interp::run(program, Arc::new(Mutex::new(Terminal)), store) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("{path}:{e}");
                    false
                }
            }
        })
    } else if command == "fmt" && fragment {
        lower::parse_fragment(&source)
            .map(|ast| print!("{ast}"))
            .map(|_| true)
    } else if command == "fmt" {
        lower::parse_program(&source)
            .map(|ast| print!("{ast}"))
            .map(|_| true)
    } else if fragment {
        parser::parse_fragment(&source)
            .map(|tree| print!("{tree}"))
            .map(|_| true)
    } else {
        parser::parse_program(&source)
            .map(|tree| print!("{tree}"))
            .map(|_| true)
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{path}:{e}");
            ExitCode::FAILURE
        }
    }
}
