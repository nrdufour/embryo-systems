use std::io::Read;
use std::process::ExitCode;

use embryo_systems::adt81::diagnostic::Severity;
use embryo_systems::adt81::{lexer, lower, names, parser, typeck};

const USAGE: &str = "usage: embryo-systems lex FILE
       embryo-systems parse [--program | --fragment] FILE
       embryo-systems fmt [--program | --fragment] FILE
       embryo-systems check FILE

FILE - reads standard input. parse prints the syntax tree, fmt the source
in canonical form; both read a program unless told --fragment. check
reports undeclared and misused names, and type errors, in a program, and
fails on errors.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (command, fragment, path) = match args.as_slice() {
        [command @ ("lex" | "check"), path] => (*command, false, *path),
        [command @ ("parse" | "fmt"), "--program", path] | [command @ ("parse" | "fmt"), path] => {
            (*command, false, *path)
        }
        [command @ ("parse" | "fmt"), "--fragment", path] => (*command, true, *path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
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
            let resolution = names::resolve(&program);
            let mut diagnostics = typeck::check_types(&program, &resolution);
            diagnostics.extend(resolution.diagnostics);
            diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
            for d in &diagnostics {
                println!("{path}:{d}");
            }
            diagnostics.iter().all(|d| d.severity != Severity::Error)
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
