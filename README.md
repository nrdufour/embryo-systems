# embryo-systems

Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust.
The scans live in `embryo-systems-archive` (private); the site is
`embryo-systems.info`.

## ADT'81

The language of HBDS. `adt81/grammar.lark` is its grammar, derived from the
polycopié's BNF and code; [docs/adt81.md](docs/adt81.md) tells how, and every
decision taken on it. `src/adt81` is the Rust implementation, a lexer, a
parser and a typed tree so far: `embryo-systems parse FILE` prints a
program's syntax tree, `embryo-systems fmt FILE` the program in canonical
form, `embryo-systems check FILE` its undeclared and misused names and
its type errors.

## Develop

`direnv allow` once, then `just` lists every recipe. `just check` is what CI runs.
