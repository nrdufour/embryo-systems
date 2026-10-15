# The HBDS store

What ADT'81 programs create and query: classes, their attributes and
links, objects, their values and links. The print builds it in three
layers:

- **GTAD**, the a.d.t. engine, in memory (pages 45 to 48);
- **ZE**, the exchange zone, a write-through cache of the persistent a.d.t.
  (page 103);
- **SFSI**, an indexed-sequential file system, with its catalogue,
  granules, index blocks and DAYFILE journal (pages 86 to 102).

`crates/hbds` is the first layer, made durable by a journal in the DAYFILE's
spirit. A faithful SFSI and ZE are a project of their own, as they are in
the print (project 13); they can come later behind the same interface.

## Languages

HBDS is an engine; ADT'81 is its first language, not its only one. The
workspace keeps them apart, and the compiler keeps them so:

- **`crates/hbds`** depends on nothing. It offers the store (classes,
  objects, attributes, links, states, hyperclasses, associations), the
  values the store keeps (`value.rs`: numbers of every kind, fuzziness,
  lists, arrays, a.d.t.), specifications (`spec.rs`), the functors of the
  catalogue (`functors.rs`), and the journal. It names no language.
- **`crates/adt81`** depends on `hbds`: its grammar, parser, checker and
  interpreter, and what is ADT'81's own: the arithmetic of values and how
  fuzziness propagates through it, the math functions, the meaning of an
  unknown name.
- **A next language** is a crate beside `adt81`, depending on `hbds`. The
  code an a.d.t. carries (COND, ACTION) is stored with the name of the
  language it is written in, `ADT'81` for ours, so that a store may hold
  the code of several, and each runs its own.
- **The CLI**, `embryo-systems`, depends on both, and picks the language
  by the file's extension: `.adt` for ADT'81.

Decided on 2026-10-15: the boundary is the workspace's, not a convention:
`hbds` cannot name `adt81`, which depends on it (cargo refuses the cycle).
Journals written before that day do not open: a property record gained
its language.

## Decisions

Taken on 2026-10-15:

- **Storage: the model in memory, a journal on disk.** Every change is one
  record, appended to `DIR/journal` and synced before the statement that
  made it goes on, as ZE forces its writes (page 103). Opening a store
  replays its journal through the same code that recorded it. A last line
  cut short by a crash is dropped, with a warning; a corrupt line elsewhere
  stops the opening, naming the line. Compaction is for later.
- **Lifetime: `run FILE` uses a store that lasts the run; `run --store DIR
  FILE` opens, or creates, a persistent one**, so that what a program
  builds is there for the next. `embryo-systems store DIR` prints what a
  store holds.
- **A.d.t. names compare case-insensitively** (decided on the same day):
  classes, objects, attributes and link relationships are kept as written
  and found folded, `"arc"` and `"ARC"` being one class. The polycopié's
  code needs it: page 127 reaches arcs by `OPSCRC("direct", "arc")` and
  computes their length in `FUNCTION "ARC" . "LENGTH"`, page 128 reads
  `p . "y"` beside `p . "X"` and declares `LA` beside `la`. It is the
  language's rule for names, carried over to the store.
- **COND and ACTION are stored, not run.** LET checks that a COND parses as
  an expression and an ACTION as statements, and keeps the text. Running
  them waits for its own step, and for the question of what "each time the
  a.d.t. is invoked" (page 156) means: read, write, link?

## The model

- **Classes** are named by strings. An object is named within its class,
  `"Paris" [ "TOWN" ]`. A class is a class, an embryo (DESIGN), a
  prototype (`COMPOSE p WITH embryos`) or a structure (`BUILD s AS p`).
- **Links between classes** come in pairs: creating `"RIVER" "crosses"
  "TOWN"` creates the reverse link at once, named by REVERSE or unnamed
  ("the system immediately creates the reverse link, which will receive
  the reverse relationship, either immediately if given by the user, or
  later, or even never", page 74). A REFLEXIVE link is its own reverse:
  `"neighbor"` both ways.
- **Links between objects** realize a link of their classes: ASSERT makes
  one, with its probability if `%` gives one, and its reverse with it;
  `ASSERT ... NOT` removes it. `a "r" b` in an expression tells whether
  the link is there.
- **Attributes** are created on classes; objects carry their values (LET
  sets one). An attribute specified COMPUTED, or with a `FUNCTION "CLASS" .
  "ATTRIBUTE"` in the program, is computed when read, by that function,
  THIS being the object (pages 127 and 128).
- **States**: HIBERN and AWAKE, DESTROY and RESUR, and PURGE, which removes
  a destroyed a.d.t. for good, with what depends on it. A hibernating or
  destroyed a.d.t. is invisible to the functors and to lookup by name, and
  so is what depends on it: an object of a hibernating class, a link to a
  destroyed object.
- **Hyperclasses**: INCLUDE puts classes in a hyperclass, EXCLUDE takes
  them out; CONT and CONTOF go down and up.
- **Associations** (ASSOC, DISSOC, page 24, figure 9) are kept as pairs;
  no functor reads them yet.
- **Functors**: all those of the catalogue (pages 76 to 80) on the model,
  each applied to an a.d.t. or to a list of them, the results united in
  order without repeats. SIGMA adds the values of the attributes in a list
  (page 78).

## The journal

One change per line, as terms: words, numbers, quoted strings (`\"`,
`\\`, `\n` escaped), and parenthesised lists.

```
class 1 "TOWN" class
attribute 3 1 "INHABITANTS"
classlink 5 6 2 "crosses" 1 "crossed by"
object 7 1 "Lyon"
set 7 3 (i 522000)
set 7 4 (fz (f 284.5) 0.5 - -)
objectlink 9 10 5 8 7 0.9
specify (ca 4) "FLOAT APPR" "kelvin" false
property (ca 4) "COND" "ADT'81" "THIS . T > 0."
state (o 8) destroyed
```

Values: `(i n)`, `(r n d)`, `(f x)`, `(g a b)`, `(x re im)`, `(q r i j k)`,
`(b true)`, `(ch "c")`, `(s "text")`; a fuzzy one `(fz value appr prob
subj)`, `-` for what it lacks; `(many ...)`, `(list cursor circular
...)`, `(array rank (lows) (dims) ...)`, `(adt (o 7))`.

## Open questions

- **What BUILD instantiates.** A prototype is made of embryos (figure 8),
  and "the prototype network has more than 50 structures instantiated";
  whether building one copies the embryos' attributes and links, and how
  the structure's classes are named, the print does not say. BUILD records
  the structure and its prototype.
- **A COND cannot quote an attribute's name**, a string holding no quote:
  page 158's `"THIS."temp" > 0."` is not one string.
- Processes (ACTIVATE...) belong to the simulation engine (page 112).

## Files

| path | what |
|---|---|
| `crates/hbds/src/store.rs` | the model, its changes, lookups |
| `crates/hbds/src/functors.rs` | the catalogue of functors, and their implementation |
| `crates/hbds/src/value.rs`, `spec.rs` | values and specifications, as the store keeps them |
| `crates/hbds/src/journal.rs` | the journal's format, recording and replay |
| `crates/adt81/src/interp.rs` | the a.d.t. statements and expressions, on the store |
| `crates/adt81/tests/corpus/run/store/` | programs on the store, with their expected output |
| `tests/store.rs` | the store across runs, a torn journal, a corrupt one |
