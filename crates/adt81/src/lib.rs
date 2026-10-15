//! ADT'81, the programming language of HBDS. The grammar is
//! `crates/adt81/grammar.lark`; docs/adt81.md tells how it was derived.

pub mod ast;
pub mod builtins;
pub mod diagnostic;
pub mod interp;
pub mod lexer;
pub mod lower;
pub mod names;
pub mod parser;
pub mod print;
pub mod sample;
pub mod typeck;
pub mod types;
pub mod uses;
pub mod value;

/// Both layers of the checker, names then types, their diagnostics sorted
/// by position.
pub fn check(program: &ast::Program) -> Vec<diagnostic::Diagnostic> {
    let resolution = names::resolve(program);
    let mut diagnostics = typeck::check_types(program, &resolution);
    diagnostics.extend(resolution.diagnostics);
    diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
    diagnostics
}
