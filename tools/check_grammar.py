#!/usr/bin/env python3
"""Gate for adt81/grammar.lark.

- the grammar loads, and every rule is reachable from `start` (Lark
  ignores a rule nothing refers to);
- every file in tests/adt81/programs parses as `program`, every file in
  tests/adt81/fragments as `fragment`, each in exactly one way;
- every line of tests/adt81/reject.txt (blank lines and # comments aside)
  is refused as a `fragment`.

`check_grammar.py FILE...` parses other files as fragments, for trying
things; `--program` before them parses them as programs.
"""

import re
import sys
from pathlib import Path

from lark import Lark
from lark.exceptions import LarkError

ROOT = Path(__file__).resolve().parent.parent
GRAMMAR = ROOT / "adt81" / "grammar.lark"
TESTS = ROOT / "tests" / "adt81"
RULE = re.compile(r"^\??([a-z_]+):(.*?)(?=^\??[a-z_]+:|^[A-Z_]+:|\Z)", re.M | re.S)


def unreachable(grammar):
    """Rules that no other rule names, start aside."""
    defined, used = set(), set()
    for m in RULE.finditer(grammar):
        defined.add(m.group(1))
        body = re.sub(r'"[^"]*"', "", re.sub(r"//[^\n]*", "", m.group(2)))
        used.update(re.findall(r"\b[a-z_]+\b", body))
    return sorted(defined - used - {"start"})


def parse(parser, path, start):
    """None if the file parses one way, else what went wrong."""
    try:
        tree = parser.parse(path.read_text(encoding="utf-8"), start=start)
    except LarkError as e:
        lines = str(e).strip().splitlines()
        return "\n    ".join(lines[:3])
    for node in tree.iter_subtrees_topdown():
        if node.data == "_ambig":
            readings = " or ".join(str(getattr(c, "data", c)) for c in node.children)
            line = next(node.scan_values(lambda v: hasattr(v, "line")), None)
            return f"line {getattr(line, 'line', '?')}: ambiguous, reads as {readings}"
    return None


def main():
    grammar = GRAMMAR.read_text(encoding="utf-8")
    parser = Lark(grammar, parser="earley", lexer="basic", start=["program", "fragment"], ambiguity="explicit")
    args = sys.argv[1:]
    if args:
        start = "program" if args[0] == "--program" else "fragment"
        targets = [(Path(a), start) for a in args if a != "--program"]
        rejects = []
    else:
        targets = [(f, "program") for f in sorted((TESTS / "programs").glob("*.adt"))]
        targets += [(f, "fragment") for f in sorted((TESTS / "fragments").glob("*.adt"))]
        rejects = [
            line
            for line in (TESTS / "reject.txt").read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]
    problems = [f"grammar.lark: {r} is never used" for r in unreachable(grammar)]
    for path, start in targets:
        err = parse(parser, path, start)
        if err:
            problems.append(f"{path.name}: {err}")
    for text in rejects:
        try:
            parser.parse(text, start="fragment")
            problems.append(f"reject.txt: accepted {text!r}")
        except LarkError:
            pass
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"grammar: {len(targets)} file(s) parse one way, {len(rejects)} reject(s) refused")


if __name__ == "__main__":
    main()
