use std::io::Read;
use std::process::ExitCode;

use embryo_systems::adt81::{lexer, lower, parser};

const USAGE: &str = "usage: embryo-systems lex FILE
       embryo-systems parse [--program | --fragment] FILE
       embryo-systems fmt [--program | --fragment] FILE

FILE - reads standard input. parse prints the syntax tree, fmt the source
in canonical form; both read a program unless told --fragment.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (command, fragment, path) = match args.as_slice() {
        ["lex", path] => ("lex", false, *path),
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
    let result = if command == "lex" {
        lexer::tokenize(&source).map(|tokens| {
            for t in tokens {
                println!("{}:{}\t{:?}\t{}", t.line, t.column, t.kind, t.text);
            }
        })
    } else if command == "fmt" && fragment {
        lower::parse_fragment(&source).map(|ast| print!("{ast}"))
    } else if command == "fmt" {
        lower::parse_program(&source).map(|ast| print!("{ast}"))
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
