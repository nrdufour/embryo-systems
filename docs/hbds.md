# The HBDS engines

What ADT'81 programs create and query: classes, their attributes and
links, objects, their values and links, and the engines that keep and
query them. The print builds the store in three layers:

- **GTAD**, the a.d.t. engine, in memory (pages 45 to 48);
- **ZE**, the exchange zone, a write-through cache of the persistent a.d.t.
  (page 103);
- **SFSI**, an indexed-sequential file system, with its catalogue,
  granules, index blocks and DAYFILE journal (pages 86 to 102).

`crates/hbds-gtad` is the first layer, made durable by a journal in the
DAYFILE's spirit (`crates/hbds-journal`). A faithful SFSI and ZE are a
project of their own, as they are in the print (project 13); they can
come later behind the same interface.

## Engines and languages

The print builds HBDS as engines (*moteurs*), each a project of its own,
for a team of its own: GTAD (project 12), SFSI and ZE (13), the list
engine (14), the functor engine (15), the expert systems (16), the
simulation (17), the neurons (18), the dialog (19). So does this
repository: each engine is a crate, as independent as it can be, and a
language is a crate that uses them. Decided on 2026-10-15.

| crate | what | depends on |
|---|---|---|
| `hbds-core` | the shared vocabulary: values (numbers of every kind, fuzziness, lists, arrays, a.d.t.), a.d.t. references, types and kinds, specifications, functor signatures; data, no logic | nothing |
| `hbds-values` | what operations do to values, whatever the language: every number type and the widening between them, comparisons, text, arrays (page 153), fuzziness propagated (pages 152, 153); `Op`, the operations a language maps its operators onto | core |
| `hbds-lists` | the list engine (project 14, page 80): cursors, circular lists, the list functors, union, intersection, complement; SIGMA adds as hbds-values does | core, values |
| `hbds-maps` | the persistent ordered map the store keeps its model in: a B-tree whose versions share nodes, so a transaction's snapshot costs nothing | nothing |
| `hbds-gtad` | the a.d.t. engine (project 12): the model, its changes, its indexes and lookups, states; persistence behind its `Recorder` trait | core, maps |
| `hbds-functors` | the functor engine (project 15): the catalogue on a.d.t. and its implementation on GTAD | core, gtad |
| `hbds-journal` | GTAD's persistence, a `Recorder`: the journal below; SFSI and ZE would be another | core, gtad |
| `hbds-simulation` | the discrete simulation (project 17): PROCESS, the sequencing set and figure 52's transitions; a language runs the bodies | core, gtad |
| `hbds-rules` | the expert systems (project 16): rules and facts, SES's functors, CHAV and HYPOCHAV; a language runs the rules' code behind its `Host` trait | core, gtad |
| `adt81` | the first language: grammar, parser, checker, interpreter, and what is the language's own (its operators mapped onto HBDS's operations, math functions, what an unknown name means); it wires the engines together | the engines |
| `hbds-acode` | A-code, HBDS's machine code: the format, its version, its text form, and what a unit asks of its language | core, values |
| `hbds-vm` | the VM that runs A-code: fibers, their frames and operand stacks as data, the store's operations, code the store keeps compiled when it runs | core, values, lists, acode, gtad, functors, rules, simulation |
| `hbds-http` | the daemon's wire: HTTP/1.1, WebSocket, JSON | none |
| `hbds-daemon` | the daemon: the executor owning the store, clients' sessions in turns over a Unix socket, and the client side; no language of its own | acode, gtad, http, vm |
| `embryo-systems` | the command; picks the language by the file's extension, `.adt` for ADT'81 | adt81, gtad, journal, acode, vm, daemon |
| `embryo-systems-bench` | the benchmarks, `just bench`: the programs of `bench/scenarios` on a ladder of sizes, by the reference and the VM, against `bench/work.txt` and `bench/<host>.txt` | adt81, gtad, vm |

The rules: an engine depends on the core and on as few engines as it
needs, never on a language; a language never on another language.
`tools/check_engines.py`, run by `just check`, holds what each crate may
use, and fails on anything else, or on a crate it does not list.

The code an a.d.t. carries (COND, ACTION) is stored with the name of the
language it is written in, `ADT'81` for ours, so that a store may hold
the code of several, and each runs its own. Journals written before
2026-10-15 do not open: a property record gained its language.

## Decisions

Taken on 2026-10-15:

- **Storage: the model in memory, a journal on disk.** Every change is one
  record, appended to `DIR/journal` and synced before the statement that
  made it goes on, as ZE forces its writes (page 103). Opening a store
  replays its journal through the same code that recorded it. A last line
  cut short by a crash is dropped, with a warning; a corrupt line elsewhere
  stops the opening, naming the line. Since 2026-10-16 (version 2 of the
  journal) each transaction ends with a `commit` line, and one a crash
  left without it is dropped whole; in the daemon a thread of its own
  syncs what queues up together, and the journal is compacted, rewritten
  as the shortest one rebuilding the store (`embryo-systems compact DIR`).
- **One opening at a time.** An open store holds a lock on `DIR/lock`, so
  a second run, or `store DIR`, on it is refused instead of writing the
  same journal. The system releases the lock when the process ends, even
  in a crash, so there is no stale lock to clear. The lock is a file of
  its own because compaction will replace the journal with a new file.
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
- **COND and ACTION run on change**; see below.
- **Setting a value to the one it has changes nothing**, and records
  nothing; its COND and ACTION still run.

## Inheritance

A class inherits the attributes and the class links of the hyperclasses
that include it, and of theirs, at any depth (decided on 2026-10-15):
figure 46 has every rule class "inheriting the components" of the
hyperclass RULE, and page 159 every class included in CONSTRAINT inherit
its FORMULA. An object carries values of its class's inherited attributes
and realizes its inherited links; lookups try the class first, then its
hyperclasses, nearest first. ATT stays a class's own attributes (page
76). A class may have an attribute of the same name as an inherited one,
and its own is found first.

## COND and ACTION

An a.d.t. may carry a COND, a boolean expression, and an ACTION, a list of
statements, both strings (pages 156 to 158): "each time the a.d.t. is
invoked, the associated condition is implicitly tested", and the action
"is executed only if the condition is verified". Decided on 2026-10-15:

- **Invoked means changed.** Three changes run them: creating an object
  runs its class's; setting an attribute's value runs the class
  attribute's; asserting a link between objects runs the class link's
  (page 158: "tested any time a new link between objects will be
  instantiated"). Then the object's own. THIS is the object concerned:
  the one created, whose attribute is set, from which the link goes.
  Reading runs nothing.
- **A false COND refuses the change**, as page 158's constraints read (a
  temperature above zero, at most six neighbors): the statement fails
  with a runtime error naming the COND, and the store is as it was before
  it. Its ACTION does not run (page 157).
- **A statement is a transaction.** Its changes, and those of the
  ACTIONs they run, are kept together or refused together: GTAD applies
  them at once, records them only when the statement succeeds, in one
  write, and otherwise puts the model back. A refused change never
  reaches the journal.
- **An ACTION's own changes run CONDs and ACTIONs in turn**, 64 deep at
  most; past that, an error.
- **Code runs in its language.** COND and ACTION are stored with the
  language they are written in; ADT'81 runs its own, and says so of any
  other.

A statement that changes the store runs as one transaction:

```mermaid
sequenceDiagram
    participant S as statement
    participant G as GTAD
    participant T as COND / ACTION
    participant J as journal
    S->>G: begin
    S->>G: change (create, set, link)
    G-->>S: applied, not yet recorded
    S->>T: run the COND of the a.d.t. changed, THIS the object
    alt the COND holds
        T->>G: the ACTION's changes (nested, 64 deep at most)
        S->>G: end, keep
        G->>J: every change, in one write
    else the COND is false, or an error
        S->>G: end, refuse
        G-->>G: the model as it was at begin
    end
```

A COND names an attribute with a quote written twice inside its string,
`"THIS . ""temp"" > 0."`; page 158's own `THIS."temp" > 0. "` is not one
string ([adt81.md](adt81.md), "Decisions").

## Expert systems

SES, HBDS's structured expert system (project 16, pages 106 to 111, with
page 80 and figures 39 and 46 to 50), and its two engines: CHAV, forward
chaining, and HYPOCHAV, which starts from hypotheses. The print gives the
model; the syntax and the algorithm are ours, decided on 2026-10-15.

```mermaid
flowchart LR
    FACT[["FACT"]] -- "includes" --> RULE[["RULE"]]
    RULE -- "includes" --> RC["a rule class"]
    RC -- "has" --> R(("a rule"))
    R -- "PREMISES read<br/>(INRULE)" --> F1["facts consumed:<br/>classes, class attributes, class links"]
    R -- "YES and NO write<br/>(OUTRULE)" --> F2["facts produced"]
    R -- "activates, inhibits" --> R2(("another rule"))
```

- **Rules are objects, as the print says** (figure 46): an object of a
  class INCLUDEd INTO the system hyperclass RULE, which is included in
  FACT, so that rules are facts and metarules come free (figure 47).
  FACT and RULE are created the first time a program names them.
- **A rule's parts are its code**, set as COND is: `LET r ' PREMISES :=
  "..."`, with PROLOGUE, PREMISES (a boolean expression), YES and NO (the
  *conséquents oui* and *non*) and EPILOGUE (statements), THIS being the
  rule. It inherits from RULE the attributes PRIORITY, RELIABILITY,
  EXPLANATION and TRIGGERING, and the links "activates" and "inhibits".
- **Facts are classes, class attributes and class links** (figure 50:
  "in fact the realizations of these components"): a created object, a
  set value, an asserted link realize them.
- **Compiled at capture** (page 80): when a part is written, ADT'81 reads
  in it the names of what it uses, the PREMISES' reads, the other parts'
  writes, and refuses a part that does not parse. The names are resolved
  against the store when needed, so that a rule written before its schema
  still connects. INRULE gives a rule's consumed facts, OUTRULE those YES
  and NO produce, INFACT a fact's producing rules, OUTFACT its consuming
  ones; a value stands for its class attribute, an object for its class.
  Only names written as strings count.

`CALL CHAV("RULES", ...)` runs the rules of those classes or
hyperclasses, every rule in RULE without one:

```mermaid
flowchart TD
    A["every rule on the agenda"] --> P{"a rule on it?"}
    P -- no --> E["done"]
    P -- "yes: the highest PRIORITY,<br/>the oldest among equals" --> T["PROLOGUE; PREMISES;<br/>YES if they hold, else NO; EPILOGUE"]
    T --> C["the facts its changes realize put<br/>the rules that consume them back"]
    C --> L["if it fired: activated rules back,<br/>inhibited ones out for the run"]
    L --> P
```

A run is one transaction, refused whole by an error, or past 10 000 tries
("CHAV does not converge"). A change to nothing new triggers nothing, and
the values a rule's PREMISES asked for do not put it back.
`CALL HYPOCHAV("statements", "RULES", ...)` first runs the statements,
whose facts are hypothetical; the rules they trigger go before all others,
and what those establish is hypothetical in turn (page 107). It prints
every hypothetical fact with the rule that established it, then puts the
store back as it was: a hypothesis is explored, not kept.

The ten parameters page 106 asks the algorithm to take into account:

| parameter | here |
|---|---|
| askability (*demandabilité*) | `ASKABLE`, a boolean expression a fact carries, THIS the object. A value the PREMISES lack is asked (`boiler[PLANT].temperature ? `) and read from the input if its fact is askable; otherwise it is not known, and the PREMISES do not hold. |
| editability (*éditabilité*) | `EDITABLE`, likewise: what YES or NO establishes of an editable fact is printed, with the rule and its EXPLANATION. |
| priority | the rule's PRIORITY, 0 by default; read at each choice, so that a metarule can change it |
| modifiability (*modifiabilité*) | `MODIFIABLE`, likewise, true by default: a fact that is not modifiable refuses any change but a rule's, at any time, as a false COND does |
| a fact's reliability | the probability of its value (`%`) or of its link, 1 without one |
| a rule's reliability | its RELIABILITY, 1 by default: what YES or NO writes takes the probability RELIABILITY times the lowest reliability the PREMISES read, when that is below 1 |
| simple triggering | by default: any change to a fact a rule consumes puts it back |
| multiple triggering | `TRIGGERING := "MULTIPLE"`: only once every fact it consumes has changed since its last try; one group of facts per rule, a simplification of figure 47's GFCR |
| activation | the "activates" links of a rule that fires |
| inhibition | its "inhibits" links |

## Simulation

In the daemon (docs/architecture.md, decision 6) the store has one
sequencing set, a kernel driving it at a pace set live; a client takes
part once it acts on time, and its processes outlive it.

The discrete simulation engine (project 17, pages 112 to 115 and 125),
SIMULA 67's, which the print names: processes are objects of classes
included in the system hyperclass PROCESS, created the first time a
program names it (figure 51). PROCESS has STATE and CLOCK; a process's
BODY is a coroutine, and its LSC, the "Local Sequence Counter", the place
the coroutine stopped. A sequencing set orders the processes by the time
they are due; the engine keeps it, and the time. Decided on 2026-10-15:

- **A class's body is `COROUTINE "PUMP"`**, named by the class's string
  ([adt81.md](adt81.md), "Decisions"); a class without one inherits its
  nearest hyperclass's. A body takes no parameters; THIS is the process,
  and each process has its own locals, its LSC being where its thread
  stopped.
- **The main program is a process**, as in SIMULA: it runs first, at time
  0, and its `HOLD FOR t` lets the others run until it is due again. When
  it ends, the simulation ends, whatever is still due. It is neither
  passivated nor terminated: nothing could reactivate it.
- **`HOLD FOR dt` holds the running process** (page 125); `HOLD x FOR dt`
  only if x is the one running.
- **First in, first out among equal times**: a process due when others
  are goes behind them (page 114, REACTIVATE).
- **STATE and CLOCK are the engine's while the run lasts**, and LET does
  not set them. CLOCK is when a process is due while it holds, when it
  last ran otherwise. Any module reads the time as `TIME`, 0 before
  anything holds; a body names its process as THIS, `PRINT THIS`.
- **Processes outlive their run, not their sequencing set**: when the main
  program ends, each process's STATE and CLOCK are stored, one not
  terminated as PASSIVE; reactivated in a later run, its body starts
  again from the top. Each run's time starts at 0. Nothing is journalled
  per event.

```mermaid
stateDiagram-v2
    [*] --> Passive: created
    Passive --> Hold: ACTIVATE, REACTIVATE [AT, BEFORE, AFTER]
    Hold --> Active: its time comes
    Active --> Hold: HOLD FOR dt
    Hold --> Hold: DELAY FOR dt
    Hold --> Passive: CANCEL
    Active --> Passive: PASSIVATE
    Active --> Terminated: TERMINATE, the end of its body
    Hold --> Terminated: KILL
    Passive --> Terminated: KILL
    Terminated --> [*]
```

Every other move is an error that says which state the process is in.
The main program holding hands time over:

```mermaid
sequenceDiagram
    participant M as main program
    participant S as sequencing set
    participant P as fire hydrant
    participant Q as surpressor
    M->>S: ACTIVATE both, at 0
    M->>S: HOLD FOR 10: due at 10
    S->>P: 0: runs its body
    P->>S: HOLD FOR 4: due at 4
    S->>Q: 0: runs its body
    Q->>S: HOLD FOR 3: due at 3
    S->>Q: 3: goes on where it held
    S->>P: 4: goes on where it held
    Note over S: ... until the main program is due
    S->>M: 10: goes on after its HOLD
```

## The model

The six basic a.d.t. (page 46, figure 6), as the store keeps them: a
class carries attributes and links to other classes; an object belongs to
a class, carries values for its class's attributes, and links realizing
its class's links. Every link has its reverse.

```mermaid
flowchart LR
    C[class] -- "has" --> A["class attribute"]
    C -- "class link<br/>and its reverse" --> D["another class"]
    O[object] -- "belongs to" --> C
    O -- "has a value of" --> OA["object attribute"]
    OA -- "realizes" --> A
    O -- "object link<br/>and its reverse" --> P["another object"]
    P -- "belongs to" --> D
    H[hyperclass] -- "includes" --> C
```

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
property (ca 4) "COND" "ADT'81" "THIS . \"TEMP\" > 0."
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
- **What associations are for.** ASSOC and DISSOC keep their pairs (page
  24, figure 9's multi-layer system), but no functor reads them yet.
- **Waiting lists.** PASSIVATE may first put a process in a waiting list
  (page 114, figure 52's INTO and OUTOF); bodies share no variables, so a
  list one fills and another empties needs a home the print does not
  give.
- **`REACTIVATE ALL x/ ... ENDALL`** (page 125) acts on every process a
  condition selects; the BNF has no such statement.

## Files

| path | what |
|---|---|
| `crates/hbds-gtad` | the model, its changes, lookups |
| `crates/hbds-functors`, `crates/hbds-lists` | the functors on a.d.t., and the list engine |
| `crates/hbds-core` | values, specifications, the vocabulary |
| `crates/hbds-values` | arithmetic, widening, comparisons and fuzzy propagation on values |
| `crates/hbds-journal` | the journal's format, recording and replay |
| `crates/hbds-rules` | the expert systems: the model, the functors, the chaining |
| `crates/hbds-simulation` | the simulation: PROCESS, the sequencing set |
| `crates/hbds-simulation/tests/sequencing.rs` | every transition of figure 52, legal and not |
| `crates/hbds-rules/tests/engine.rs` | the chaining rule by rule, on a fake language |
| `tools/check_engines.py` | what each crate may depend on |
| `crates/bench` | the benchmarks' harness: inputs, expected outputs, the gate |
| `bench/scenarios/` | the benchmarks' programs |
| `crates/adt81/src/interp.rs` | the a.d.t. statements and expressions, on the store |
| `crates/adt81/tests/corpus/run/store/` | programs on the store, with their expected output |
| `crates/adt81/tests/store.rs` | the store across runs, a torn journal, a corrupt one, a store already open, a refused change, code in another language |
