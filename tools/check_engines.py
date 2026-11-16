#!/usr/bin/env python3
"""Gate for the engines' independence (CLAUDE.md, docs/hbds.md "Engines").

Each crate may depend only on the workspace crates this table allows:
engines on the shared core and on as few engines as they need, never on a
language; a language on engines, never on another language. A new crate
must be added here, with what it may use, or the gate fails. The table is
held to the rule too, so that it cannot allow what the rule forbids, and
to the crates, so that it allows no more than they use. A crate's tests
may use what TESTS adds, as dev-dependencies.
"""

import json
import subprocess
import sys

ALLOWED = {
    "hbds-core": set(),
    "hbds-maps": set(),
    "hbds-values": {"hbds-core"},
    "hbds-acode": {"hbds-core", "hbds-values"},
    "hbds-vm": {"hbds-core", "hbds-values", "hbds-lists", "hbds-acode", "hbds-gtad", "hbds-functors", "hbds-rules", "hbds-simulation", "hbds-reflexes", "hbds-constraints", "hbds-dialog", "hbds-neurons"},
    "hbds-lists": {"hbds-core", "hbds-values"},
    "hbds-http": {"hbds-core"},
    "hbds-daemon": {"hbds-core", "hbds-acode", "hbds-gtad", "hbds-http", "hbds-journal", "hbds-vm"},
    "hbds-gtad": {"hbds-core", "hbds-maps"},
    "hbds-functors": {"hbds-core", "hbds-gtad"},
    "hbds-journal": {"hbds-core", "hbds-gtad"},
    "hbds-rules": {"hbds-core", "hbds-gtad"},
    "hbds-simulation": {"hbds-core", "hbds-gtad"},
    "hbds-reflexes": {"hbds-core", "hbds-gtad"},
    "hbds-constraints": {"hbds-core", "hbds-gtad"},
    "hbds-dialog": {"hbds-core", "hbds-gtad"},
    "hbds-neurons": {"hbds-core", "hbds-gtad", "hbds-reflexes"},
    "adt81": {"hbds-core", "hbds-values", "hbds-acode", "hbds-lists", "hbds-gtad", "hbds-functors", "hbds-rules", "hbds-simulation", "hbds-reflexes", "hbds-constraints", "hbds-dialog", "hbds-neurons"},
    "embryo-systems": {"adt81", "hbds-gtad", "hbds-journal", "hbds-acode", "hbds-vm", "hbds-daemon"},
    "embryo-systems-bench": {"adt81", "hbds-gtad", "hbds-vm"},
}


LANGUAGES = {"adt81"}
COMMANDS = {"embryo-systems", "embryo-systems-bench"}

# What a crate's tests may use beyond ALLOWED, as dev-dependencies: the
# crate itself stays as independent as the table says.
TESTS = {
    "adt81": {"hbds-journal"},
    "embryo-systems": {"hbds-http"},
}


def table(members):
    """The table against its own rule, whatever the crates declare."""
    problems = []
    for name, allowed in sorted(ALLOWED.items()):
        if name not in members:
            problems.append(f"{name}: in tools/check_engines.py, not in the workspace")
        for dep in sorted(allowed | TESTS.get(name, set())):
            if dep not in ALLOWED:
                problems.append(f"{name} is allowed {dep}, which the table does not know")
        for dep in sorted(allowed & COMMANDS):
            problems.append(f"{name} is allowed {dep}: nothing depends on a command")
        if name in COMMANDS:
            continue
        for dep in sorted(allowed & LANGUAGES - {name}):
            kind = "a language" if name in LANGUAGES else "an engine"
            problems.append(f"{name}, {kind}, is allowed {dep}, a language")
    for name in sorted(set(TESTS) - set(ALLOWED)):
        problems.append(f"{name}: in TESTS, not in ALLOWED")
    return problems


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
    problems = table(members)
    for name, package in sorted(members.items()):
        if name not in ALLOWED:
            problems.append(f"{name}: not in tools/check_engines.py; say what it may depend on")
            continue
        used, tested = set(), set()
        for dep in package["dependencies"]:
            if dep["name"] not in members:
                continue
            if dep["kind"] == "dev":
                tested.add(dep["name"])
                if dep["name"] not in ALLOWED[name] | TESTS.get(name, set()):
                    problems.append(f"{name}'s tests depend on {dep['name']}, which they may not")
                continue
            used.add(dep["name"])
            if dep["name"] not in ALLOWED[name]:
                problems.append(f"{name} depends on {dep['name']}, which it may not")
        for dep in sorted(ALLOWED[name] - used):
            problems.append(f"{name} is allowed {dep} and does not use it: take it out of the table")
        for dep in sorted(TESTS.get(name, set()) - tested):
            problems.append(f"{name}'s tests are allowed {dep} and do not use it: take it out of TESTS")
    if problems:
        print("\n".join(problems))
        sys.exit(1)
    print(f"engines: {len(members)} crates, each depending only on what it may")


if __name__ == "__main__":
    main()
