use std::io::{BufRead, Read, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

mod build;

use adt81::compile::{Adt81, compile};
use adt81::diagnostic::Severity;
use adt81::{interp, lexer, lower, parser};
use hbds_gtad::Store;
use hbds_journal as journal;

const USAGE: &str = "usage: embryo-systems lex FILE
       embryo-systems parse [--program | --fragment] FILE
       embryo-systems fmt [--program | --fragment] FILE
       embryo-systems check FILE
       embryo-systems run [--store DIR | --connect SOCKET] [--reference] FILE
       embryo-systems acode FILE
       embryo-systems exec [--store DIR | --connect SOCKET] FILE
       embryo-systems build FILE -o OUT
       embryo-systems daemon --socket SOCKET [--store DIR] [--slice N]
       embryo-systems store DIR

FILE - reads standard input, but for run. parse prints the syntax tree,
fmt the source in canonical form; both read a program unless told
--fragment. check reports undeclared and misused names, and type errors,
in a program, and fails on errors. run checks a program, then runs it:
READ reads standard input, PRINT writes standard output; its classes and
objects last the run, or go in the store in DIR, created if need be. It
runs compiled to A-code, on the VM; --reference runs it on the
interpreter, the VM's specification. acode prints a program compiled to
A-code, an .acode file's contents; exec runs one, as run does. build
writes OUT, an executable that runs the program, OUT [--store DIR |
--connect SOCKET]. daemon serves the store in DIR, or one for its life,
on SOCKET: --connect runs a program there, its clients taking turns of N
instructions (10000). store prints what the store in DIR holds.";

/// READ on standard input, PRINT on standard output.
struct Terminal;

fn read_line() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
    }
}

fn write(text: &str) {
    print!("{text}");
    let _ = std::io::stdout().flush();
}

impl interp::Io for Terminal {
    fn read_line(&mut self) -> Option<String> {
        read_line()
    }

    fn write(&mut self, text: &str) {
        write(text)
    }
}

impl hbds_vm::Io for Terminal {
    fn read_line(&mut self) -> Option<String> {
        read_line()
    }

    fn write(&mut self, text: &str) {
        write(text)
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

/// Runs `unit` on the VM, on the store in `dir` or one for the run;
/// `what` names it in errors.
fn exec(unit: &hbds_acode::Unit, dir: Option<&str>, what: &str) -> bool {
    // The language that compiles the code the store keeps: ADT'81 the
    // only one yet.
    if unit.language != hbds_acode::Language::name(&Adt81) {
        eprintln!(
            "{what}: A-code in {}, a language this embryo-systems does not have",
            unit.language
        );
        return false;
    }
    let mut store = match open_store(dir) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };
    match hbds_vm::run(unit, &mut store, &mut Terminal, &Adt81, None) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("{what}:{e}");
            false
        }
    }
}

/// ADT'81, lent to the daemon for the code its store keeps.
static ADT81: Adt81 = Adt81;

/// The A-code in `text`, run as a session of the daemon at `socket`.
fn remote(text: &str, socket: &str, what: &str) -> ExitCode {
    let stdin = std::io::stdin();
    let r = hbds_daemon::client::run(
        std::path::Path::new(socket),
        text,
        &mut std::io::stdout(),
        &mut stdin.lock(),
    );
    match r {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(e)) => {
            eprintln!("{what}:{e}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// The A-code in `text`, run on the VM.
fn exec_text(text: &str, dir: Option<&str>, what: &str) -> ExitCode {
    let ok = match hbds_acode::Unit::read(text) {
        Ok(unit) => exec(&unit, dir, what),
        Err(e) => {
            eprintln!("{what}: {e}");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `daemon --socket SOCKET [--store DIR] [--slice N]`.
fn daemon(args: &[&str]) -> ExitCode {
    let usage = || {
        eprintln!("{USAGE}");
        ExitCode::FAILURE
    };
    let (mut socket, mut dir, mut slice) = (None, None, 10_000);
    for pair in args.chunks(2) {
        match pair {
            ["--socket", s] => socket = Some(*s),
            ["--store", d] => dir = Some(*d),
            ["--slice", n] => match n.parse::<u64>() {
                Ok(n) if n > 0 => slice = n,
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    let Some(socket) = socket else {
        return usage();
    };
    let store = match open_store(dir) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match hbds_daemon::serve(store, std::path::Path::new(socket), &ADT81, slice) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // A built program is the program, not the command.
    match build::carried() {
        Ok(Some(text)) => {
            let me = std::env::args().next().unwrap_or_default();
            return match args.as_slice() {
                [] => exec_text(&text, None, &me),
                ["--store", dir] => exec_text(&text, Some(dir), &me),
                ["--connect", socket] => remote(&text, socket, &me),
                _ => {
                    eprintln!("usage: {me} [--store DIR | --connect SOCKET]");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }
    if let ["daemon", rest @ ..] = args.as_slice() {
        return daemon(rest);
    }
    if let ["exec", "--connect", socket, path] = args.as_slice() {
        return match std::fs::read_to_string(path) {
            Ok(text) => remote(&text, socket, path),
            Err(e) => {
                eprintln!("{path}: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let (args, connect) = match args.as_slice() {
        ["run", "--connect", socket, rest @ ..] => {
            let mut a = vec!["run"];
            a.extend(rest);
            (a, Some(*socket))
        }
        _ => (args.clone(), None),
    };
    let exec_args = match args.as_slice() {
        ["exec", "--store", dir, path] => Some((Some(*dir), *path)),
        ["exec", path] => Some((None, *path)),
        _ => None,
    };
    if let Some((dir, path)) = exec_args {
        return match std::fs::read_to_string(path) {
            Ok(text) => exec_text(&text, dir, path),
            Err(e) => {
                eprintln!("{path}: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let (args, out) = match args.as_slice() {
        ["build", path, "-o", out] => (vec!["build", *path], Some(*out)),
        _ => (args.clone(), None),
    };
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
    let (args, vm) = match args.as_slice() {
        ["run", "--reference", path] => (vec!["run", *path], false),
        _ => (args.clone(), true),
    };
    let (command, fragment, path) = match args.as_slice() {
        [command @ ("lex" | "check" | "acode"), path] => (*command, false, *path),
        ["build", path] if *path != "-" && out.is_some() => ("build", false, *path),
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
            if let Some(socket) = connect {
                if !vm {
                    eprintln!("{path}: --reference runs here, not in a daemon");
                    return false;
                }
                return remote(&compile(&program).text(), socket, path) == ExitCode::SUCCESS;
            }
            if vm {
                return exec(&compile(&program), store_dir, path);
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
    } else if command == "acode" || command == "build" {
        lower::parse_program(&source).map(|program| {
            let diagnostics = adt81::check(&program);
            for d in &diagnostics {
                eprintln!("{path}:{d}");
            }
            if diagnostics.iter().any(|d| d.severity == Severity::Error) {
                return false;
            }
            let text = compile(&program).text();
            let Some(out) = out else {
                print!("{text}");
                return true;
            };
            match build::write(std::path::Path::new(out), &text) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("{out}: {e}");
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
