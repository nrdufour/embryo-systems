use std::io::Read;
use std::process::ExitCode;

use embryo_systems::adt81::{lexer, parser};

const USAGE: &str = "usage: embryo-systems lex FILE
       embryo-systems parse [--program | --fragment] FILE

FILE - reads standard input. parse reads a program unless told --fragment.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (command, fragment, path) = match args.as_slice() {
        ["lex", path] => ("lex", false, *path),
        ["parse", "--program", path] | ["parse", path] => ("parse", false, *path),
        ["parse", "--fragment", path] => ("parse", true, *path),
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
    let result = if command == "lex" {
        lexer::tokenize(&source).map(|tokens| {
            for t in tokens {
                println!("{}:{}\t{:?}\t{}", t.line, t.column, t.kind, t.text);
            }
        })
    } else if fragment {
        parser::parse_fragment(&source).map(|tree| print!("{tree}"))
    } else {
        parser::parse_program(&source).map(|tree| print!("{tree}"))
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{path}:{e}");
            ExitCode::FAILURE
        }
    }
}
