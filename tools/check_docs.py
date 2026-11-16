#!/usr/bin/env python3
"""Gate for the documentation: every path it names, and every relative
link, exists. Paths are words starting with one of the repository's
directories, in prose or code; a glob must match a file. A file named
alone, in backticks, `flake.nix` or `module.nix`, is one the repository
tracks, at the root or under that name anywhere. The README's table of
crates has every member of the workspace.
"""

import glob
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = [ROOT / "README.md", ROOT / "CLAUDE.md", *sorted((ROOT / "docs").glob("*.md"))]
DIRS = "crates|docs|tools|src|tests|nix|bench|\\.forgejo"
PATH = re.compile(rf"(?<![\w/.-])((?:{DIRS})/[\w./*<>-]*[\w/*])")
# A file named alone: what the root holds, by its extension or its name.
ALONE = re.compile(r"`((?:[\w.-]+\.(?:nix|toml|lock|json5|md|yaml|py|lark)|justfile|LICENSE))`")
LINK = re.compile(r"\]\(([^)#\s]+)(?:#[^)]*)?\)")


def exists(path: str, base: Path) -> bool:
    target = base / path
    if "*" in path:
        return bool(glob.glob(str(target), recursive=True))
    return target.exists()


def crates(readme: str) -> list[str]:
    """The workspace's members the README's table of crates lacks."""
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    rows = set(re.findall(r"^\| `([^`]+)` \|", readme, re.MULTILINE))
    return [m for m in manifest["workspace"]["members"] if m not in rows]


def main():
    problems = []
    tracked = subprocess.run(
        ["git", "ls-files"], capture_output=True, text=True, check=True, cwd=ROOT
    ).stdout.split()
    names = {Path(t).name for t in tracked}
    for doc in DOCS:
        text = doc.read_text(encoding="utf-8")
        name = doc.relative_to(ROOT)
        for n, line in enumerate(text.splitlines(), 1):
            for path in PATH.findall(line):
                # `bench/<host>.txt`: one file a host.
                if not exists(re.sub(r"<\w+>", "*", path), ROOT):
                    problems.append(f"{name}:{n}: no {path}")
            for alone in ALONE.findall(line):
                if alone not in names:
                    problems.append(f"{name}:{n}: no file named {alone}")
            for link in LINK.findall(line):
                if "://" not in link and not exists(link, doc.parent):
                    problems.append(f"{name}:{n}: the link {link} leads nowhere")
    for member in crates((ROOT / "README.md").read_text(encoding="utf-8")):
        problems.append(f"README.md: its table of crates lacks {member}")
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"docs: {len(DOCS)} files, every path and link there")


if __name__ == "__main__":
    main()
