#!/usr/bin/env python3
"""Gate for the documentation: every path it names, and every relative
link, exists. Paths are words starting with crates/, docs/, tools/, src/
or tests/, in prose or code; a glob must match a file.
"""

import glob
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = [ROOT / "README.md", ROOT / "CLAUDE.md", *sorted((ROOT / "docs").glob("*.md"))]
PATH = re.compile(r"(?<![\w/.-])((?:crates|docs|tools|src|tests)/[\w./*-]*[\w/*])")
LINK = re.compile(r"\]\(([^)#\s]+)(?:#[^)]*)?\)")


def exists(path: str, base: Path) -> bool:
    target = base / path
    if "*" in path:
        return bool(glob.glob(str(target), recursive=True))
    return target.exists()


def main():
    problems = []
    for doc in DOCS:
        text = doc.read_text(encoding="utf-8")
        name = doc.relative_to(ROOT)
        for n, line in enumerate(text.splitlines(), 1):
            for path in PATH.findall(line):
                if not exists(path, ROOT):
                    problems.append(f"{name}:{n}: no {path}")
            for link in LINK.findall(line):
                if "://" not in link and not exists(link, doc.parent):
                    problems.append(f"{name}:{n}: the link {link} leads nowhere")
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"docs: {len(DOCS)} files, every path and link there")


if __name__ == "__main__":
    main()
