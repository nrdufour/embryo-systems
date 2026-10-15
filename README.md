# embryo-systems

Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust.
The scans live in `embryo-systems-archive` (private); the site is
`embryo-systems.info`.

## ADT'81

The language of HBDS. `adt81/grammar.lark` is its grammar, derived from the
polycopié's BNF and code; [docs/adt81.md](docs/adt81.md) tells how, and every
decision taken on it. `src/adt81` is the Rust implementation: a lexer, a
parser, a typed tree, a checker and an interpreter.

```sh
cargo run -q -- run tests/adt81/run/hello.adt    # Hello, world!
```

`embryo-systems run FILE` checks a program and runs it, its classes and
objects lasting the run; `run --store DIR FILE` keeps them in the HBDS
store in DIR, for the next program ([docs/hbds.md](docs/hbds.md)), and
`store DIR` shows what it holds. `check FILE` only checks a program,
`fmt FILE` prints it in canonical form, `parse FILE` prints its syntax
tree. `tests/adt81/run/` holds more programs to run, with their expected
output.

## Develop

`direnv allow` once, then `just` lists every recipe. `just check` is what CI runs.
