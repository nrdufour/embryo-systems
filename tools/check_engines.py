#!/usr/bin/env python3
"""Gate for the engines' independence (CLAUDE.md, docs/hbds.md "Engines").

Each crate may depend only on the workspace crates this table allows:
engines on the shared core and on as few engines as they need, never on a
language; a language on engines, never on another language. A new crate
must be added here, with what it may use, or the gate fails.
"""

import json
import subprocess
import sys

ALLOWED = {
    "hbds-core": set(),
    "hbds-maps": set(),
    "hbds-values": {"hbds-core"},
    "hbds-acode": {"hbds-core", "hbds-values"},
    "hbds-vm": {"hbds-core", "hbds-values", "hbds-lists", "hbds-acode", "hbds-gtad", "hbds-functors", "hbds-rules", "hbds-simulation"},
    "hbds-lists": {"hbds-core", "hbds-values"},
    "hbds-gtad": {"hbds-core", "hbds-maps"},
    "hbds-functors": {"hbds-core", "hbds-gtad"},
    "hbds-journal": {"hbds-core", "hbds-gtad"},
    "hbds-rules": {"hbds-core", "hbds-gtad"},
    "hbds-simulation": {"hbds-core", "hbds-gtad"},
    "adt81": {"hbds-core", "hbds-values", "hbds-acode", "hbds-lists", "hbds-gtad", "hbds-functors", "hbds-journal", "hbds-rules", "hbds-simulation"},
    "embryo-systems": {"adt81", "hbds-gtad", "hbds-journal", "hbds-acode", "hbds-vm"},
    "embryo-systems-bench": {"adt81", "hbds-gtad", "hbds-vm"},
}


def main():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )
    members = {p["name"]: p for p in meta["packages"]}
    problems = []
    for name, package in sorted(members.items()):
        if name not in ALLOWED:
            problems.append(f"{name}: not in tools/check_engines.py; say what it may depend on")
            continue
        for dep in package["dependencies"]:
            if dep["name"] in members and dep["name"] not in ALLOWED[name]:
                problems.append(f"{name} depends on {dep['name']}, which it may not")
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"engines: {len(members)} crates, each depending only on what it may")


if __name__ == "__main__":
    main()
