# embryo-systems

Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust.
The scans live in `embryo-systems-archive` (private); the site is
`embryo-systems.info`.

## ADT'81

The language of HBDS. `crates/adt81/grammar.lark` is its grammar, derived from the
polycopié's BNF and code; [docs/adt81.md](docs/adt81.md) tells how, and every
decision taken on it. `crates/adt81` is the Rust implementation: a lexer, a
parser, a typed tree, a checker and an interpreter.

```sh
cargo run -q -- run crates/adt81/tests/corpus/run/hello.adt    # Hello, world!
```

`embryo-systems run FILE` checks a program and runs it, its classes and
objects lasting the run; `run --store DIR FILE` keeps them in the HBDS
store in DIR, for the next program ([docs/hbds.md](docs/hbds.md)), and
`store DIR` shows what it holds. `check FILE` only checks a program,
`fmt FILE` prints it in canonical form, `parse FILE` prints its syntax
tree. `crates/adt81/tests/corpus/run/` holds more programs to run, with their expected
output.

## Layout

| crate | what |
|---|---|
| `crates/hbds-core` | the vocabulary the engines share: values, a.d.t., specs |
| `crates/hbds-gtad` | the a.d.t. engine |
| `crates/hbds-lists` | the list engine |
| `crates/hbds-functors` | the functor engine |
| `crates/hbds-journal` | GTAD's persistence |
| `crates/adt81` | ADT'81: grammar, parser, checker, interpreter |
| `.` (`src/main.rs`) | the `embryo-systems` command |

Each engine is a crate of its own, a language a crate that uses them;
`just engines` holds each to what it may depend on
([docs/hbds.md](docs/hbds.md), "Engines and languages").

## Develop

`direnv allow` once, then `just` lists every recipe. `just check` is what CI runs.
