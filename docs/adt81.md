# ADT'81: how the grammar was derived

ADT'81 is the programming language of François Bouillé's HBDS system. Its
definition is the "BNF simplifiée" printed in the 1995 polycopié, pages 19 to
28, with the abbreviations of page 29 and the notation of figure 1, page 17.
The polycopié itself is not in this repository: its copyright is under review,
so this project quotes and cites it, and carries none of it.

`adt81/grammar.lark` is our grammar of the same language. This page tells how
it was obtained, where it departs from the print, and why.

## Method

1. **Transcription.** The pages were scanned, cleaned and transcribed in a
   private archive, each page checked against its image.
2. **The print's BNF, verbatim.** Pages 19 to 28 were assembled into one file
   as printed, with every rule the print uses but never defines, defines but
   never uses, or defines twice declared explicitly.
3. **A grammar a parser accepts.** That text is not parser input: its
   brackets are overloaded, rules are missing, and the code the polycopié
   quotes does not follow it. It was rewritten as a Lark grammar, each
   departure commented with its page.
4. **Tested on the polycopié's own code.** The eleven code listings the
   polycopié quotes (pages 49-51, 76-79, 127-128, 152-158) all parse. Sixteen
   print errors in them had to be fixed first (a missing quote, a missing
   `;`, a keyword misspelt `APPRO`...), each checked on the page image.
5. **Unambiguous on purpose.** Every listing, and a test program with one of
   each construct, must parse in exactly one way. Removing the print's
   redundant alternatives took the count of ambiguous points in the listings
   from 326 to 0.
6. **Our grammar.** This repository's grammar was written afresh from that
   work, with our rule names and structure. It still accepts all eleven
   listings, unambiguously, and the Rust lexer produces the same tokens as
   Lark on all of them.

## The print's notation

Figure 1 (page 17) gives the BNF's operators through the transformations the
course's compiler-compiler (the "C2") applies to them:

| print | meaning | here |
|---|---|---|
| `[ a \| b ]` | one of ("un parmi") | `(a \| b)` |
| `[ a ]` | optional ("facultatif") | `a?` |
| `{ a }` | one or more ("itératif") | `a+` |
| `[{ a }]` | zero or more | `a*` |

Brackets also stand for themselves in a few rules (array bounds, indices, an
object's class); context tells which, and the grammar quotes those, `"["`.

## Departures from the print

### Errors of the print

| page | in the print | here |
|---|---|---|
| 20 | `MATRIX` alternative lacks its `\|`, `TENSOR` a closing bracket (a handwritten note adds both) | added |
| 20 | initial value or constant is "one of" | optional |
| 22 | `AFTER`/`BEFORE` position is "one of" | optional |
| 22, 25 | the index rule is defined twice | one rule, an expression |
| 23 | the operand list of a state change appears twice | once |
| 23 | an object link ends in an object name; a handwritten note corrects it to a link name | corrected |
| 19, 24 | the parameter specification and the `SPECIFY` statement share a name | two rules |
| 24 | `of` in lower case | `OF` |
| 25 | a stray bracket after `AT`, marked "pb" by hand | removed |
| 25, 27 | two alternatives end in a dangling `\|` | removed |
| 26 | the sign of a constant is "one of" | optional |
| 26 | the float operators read `*` and `-` | `*` and `/`, as for complex numbers |
| 27 | `AND` joins exactly two terms | any number, as `OR` |
| 27 | a functor's prefix may be a bare expression, so `a + b'F` would apply `F` to `a + b` | parenthesised, `(a + b)'F` |

### Gaps, filled by analogy

- **Comparator** (page 27, never listed): `= /= < <= > >=`.
- **A.d.t. operand** (page 27, never defined): page 29 expands the
  abbreviation as "type abstrait de donnée"; any designated class, object,
  attribute or link.
- **Right operand of `COMPL`** (page 28): a list term, as on its left.
- **Object attribute expression** (pages 26, 27): page 23's object attribute.
- **Gauss, quaternion and rational expressions** (page 27, "pas à traiter",
  not to be handled): nothing to add, since expressions are untyped.

### Constructs the polycopié's code uses

The listings are later than the BNF and use more than it defines: a
computed attribute's algorithm named after the attribute
(`FUNCTION "ARC"."LENGTH"`, pages 49-50); a functor naming its prefix
(`FUNCTOR C'COUPLE(D)`, page 76); `DESIGN` of attributes and links
(pages 49, 51); `SPECIFY` with a colon, `COMPUTED`, a `UNIT`, and on classes
and links (pages 49, 154, 158); properties of an a.d.t. such as `COND` and
`ACTION` (pages 156-158); array operators `[+] [*] [.] [^]` (page 153);
fuzzy values (pages 152-154, below); declarations and statements quoted
outside any module (the `fragment` start symbol).

### Structure

- **Expressions are untyped.** The print splits arithmetic by type (integer,
  float, complex, Gauss, quaternion, rational), and each split starts with an
  identifier whose type only its declaration knows. One expression grammar
  replaces them, by precedence, loosest first: `OR NOR XOR`, `AND NAND`,
  `NOT`, comparisons and `IN NOTIN`, `UNION`, `INTER`, `COMPL`, `&`,
  `+ -`, `* / MOD //`, `^`. The print gives no precedence across types; this
  order is ours.
- **A bare name is one rule.** The print gives each kind of a.d.t. its own
  identifier rule (page 24). With bare names the kinds read alike, so
  designations are one rule per shape (a name, `"A" . "x"`, `a "r" b`,
  `"P" [ "C" ]`), and whether a name is a class or an object is the
  checker's. The same holds for `CREATE` lists and `ASSOC`/`DISSOC` pairs.
- **A list expression after `EACH ... OF`**, page 28's own form, so that the
  `/` introducing the filter is never read as a division.
- **A functor result inside a designation** is written in parentheses,
  `LET (a'F) . "x" ' COND := ...`; otherwise `a'F'G` would read two ways.

## Decisions

The print leaves these open; they were decided on 2026-10-14.

- **Imaginary units are suffixes**, `2.3 + 3.4i`, as in Go. Page 26 writes
  them as prefixes (`i 3.4`) and page 153 says `i` "works like an operator",
  which collides with a call to a function named `i`. Considered: reserving
  `i`, `j`, `k` as prefix keywords (faithful, but no variable could be called
  `i`), or leaving the checker to tell a call from a unit (MATLAB's choice,
  and its known pitfall). The suffix needs no reserved name and no
  context.
- **`%` is a probability only.** Page 26 lists it among the integer
  operators; the fuzziness paper (page 152) uses it for the probability of a
  value. Integer division is `/`.
- **Fuzziness attaches to number literals only:** an approximation `+-`, a
  probability `%`, an opinion `@"..."`, in that order, and `{ }` for a value
  with several possibilities. Every example in the polycopié is a literal; a
  variable becomes fuzzy by declaration and assignment. The plain form is
  kept: a delimited form (backticks were considered) would clash with
  Markdown and with the `'` of functors, for no gain in parsing.
- **A link's relationship is a string literal**, as in every listing:
  `"AREA" "neighbor" "AREA"`. Page 23 allows any string expression, which
  makes three juxtaposed names a link, a boolean and more at once.
- **`LET` is required to set an a.d.t.'s property** (page 23, page 156).
  Page 158 drops it; that is read as a print error.
- **The language is case-insensitive**, names and keywords alike. *This is
  a deviation from the original document, taken deliberately.* The polycopié
  prints every keyword in upper case and underlined, and nothing in it shows
  a keyword written otherwise; we accept `loop`, `Loop` and `LOOP` as the
  same keyword. For names it follows the print: page 50 writes the functor
  `opscrc` where page 127 writes `OPSCRC`, so `xi`, `XI` and `Xi` are one
  name. It is worth it: with names folded and keywords not, `loop` would be
  a name and `LOOP` a keyword, a trap in any program; one rule for every
  word, as in Pascal or Ada, removes it. Every program the polycopié quotes
  parses the same as before, and ours keep the print's upper-case keywords.
  Considered and rejected: keywords in upper case only (the trap above), and
  keywords reserved in every case but required in upper case (stricter than
  any reader needs). Strings keep their case: `"ARC"` and `"arc"` are two
  class names.
- **`EXIT`, `NEXT`, `RELOOP` take an optional count**, one by default. Page
  22 always writes it.

## What the grammar leaves to the checker

Types: which operators apply to which operands, complex numbers compared with
`=` and `/=` only (page 27), a character constant being one character long.
Kinds: whether a name designates a variable, a class, an object, an
attribute or a link; homogeneous `CREATE` lists; which `ASSOC` pairs are
legal; a functor's prefix being an a.d.t.

## Files

| path | what |
|---|---|
| `adt81/grammar.lark` | the grammar |
| `tests/adt81/programs/*.adt` | programs, parsed from `program` |
| `tests/adt81/fragments/*.adt` | declarations and statements, from `fragment` |
| `tests/adt81/reject.txt` | statements the grammar must refuse, one per decision among them |
| `tools/check_grammar.py` | the gate: `just check-grammar` |
| `src/adt81/lexer.rs` | the Rust lexer; its tests keep its keywords and symbols in step with the grammar |

`just check` runs the gate with the rest: every test program parses one way
only, every reject is refused, every rule is reachable.
