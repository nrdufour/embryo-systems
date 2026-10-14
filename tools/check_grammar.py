#!/usr/bin/env python3
"""Gate for adt81/grammar.lark.

- the grammar loads, and every rule is reachable from `start` (Lark
  ignores a rule nothing refers to);
- every file in tests/adt81/programs parses as `program`, every file in
  tests/adt81/fragments as `fragment`, each in exactly one way;
- every line of tests/adt81/reject.txt (blank lines and # comments aside)
  is refused as a `fragment`;
- the Rust parser (`embryo-systems parse`) gives the same tree as Lark for
  every test file, and refuses every reject.

`check_grammar.py FILE...` parses other files as fragments, for trying
things; `--program` before them parses them as programs; `--tree` prints
the tree in the form `embryo-systems parse` prints it.
"""

import functools
import json
import re
import subprocess
import sys
from pathlib import Path

from lark import Lark, Tree
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


def dump(node, depth=0):
    """The tree, one node per line: a rule by its name, a token as `- text`."""
    pad = "  " * depth
    if isinstance(node, Tree):
        return [pad + node.data] + [line for c in node.children for line in dump(c, depth + 1)]
    return [f"{pad}- {node}"]


def parse(parser, path, start):
    """The tree's dump if the file parses one way, else what went wrong."""
    try:
        tree = parser.parse(path.read_text(encoding="utf-8"), start=start)
    except LarkError as e:
        lines = str(e).strip().splitlines()
        return None, "\n    ".join(lines[:3])
    for node in tree.iter_subtrees_topdown():
        if node.data == "_ambig":
            readings = " or ".join(str(getattr(c, "data", c)) for c in node.children)
            line = next(node.scan_values(lambda v: hasattr(v, "line")), None)
            return None, f"line {getattr(line, 'line', '?')}: ambiguous, reads as {readings}"
    return dump(tree), None


@functools.cache
def parser_binary():
    """The debug build's path, wherever cargo puts it (CI sets CARGO_TARGET_DIR)."""
    meta = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return Path(json.loads(meta.stdout)["target_directory"]) / "debug" / "embryo-systems"


def rust(start, path=None, text=None):
    """`embryo-systems parse`'s output lines, and whether it succeeded."""
    args = [str(parser_binary()), "parse", f"--{start}", str(path) if path else "-"]
    run = subprocess.run(args, input=text, capture_output=True, text=True)
    return run.stdout.splitlines(), run.returncode == 0


def differ(ours, theirs):
    """Where two dumps first differ, or None."""
    for i, (a, b) in enumerate(zip(ours, theirs)):
        if a != b:
            return f"tree line {i + 1}: Rust has {a.strip()!r}, Lark {b.strip()!r}"
    if len(ours) != len(theirs):
        return f"trees of {len(ours)} and {len(theirs)} lines"
    return None


def main():
    grammar = GRAMMAR.read_text(encoding="utf-8")
    parser = Lark(
        grammar,
        parser="earley",
        lexer="basic",
        start=["program", "fragment"],
        ambiguity="explicit",
        keep_all_tokens=True,
    )
    args = sys.argv[1:]
    tree = "--tree" in args
    start = "program" if "--program" in args else "fragment"
    files = [Path(a) for a in args if not a.startswith("--")]
    if tree:
        for path in files:
            lines, err = parse(parser, path, start)
            print("\n".join(lines) if lines else f"{path}: {err}")
        return
    if files:
        targets = [(path, start) for path in files]
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
        lines, err = parse(parser, path, start)
        if err:
            problems.append(f"{path.name}: {err}")
            continue
        ours, ok = rust(start, path=path)
        err = differ(ours, lines) if ok else "the Rust parser refuses it"
        if err:
            problems.append(f"{path.name}: {err}")
    for text in rejects:
        try:
            parser.parse(text, start="fragment")
            problems.append(f"reject.txt: accepted {text!r}")
        except LarkError:
            pass
        if rust("fragment", text=text)[1]:
            problems.append(f"reject.txt: the Rust parser accepts {text!r}")
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"grammar: {len(targets)} file(s) parse one way, as in Rust; {len(rejects)} reject(s) refused")


if __name__ == "__main__":
    main()
