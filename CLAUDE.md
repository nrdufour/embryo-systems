# CLAUDE.md

Guidance for Claude Code working in this repository.

## What this is

Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust:
the engines in `crates/hbds-*`, the language in `crates/adt81`. README tells
what works and how it fits together; `docs/` holds every decision.

## Rules

- **Nothing from the polycopié is carried here.** Its copyright is under
  review: this repository quotes and cites pages, never copies a scan, a
  transcription or a listing. Test programs are written for the purpose. The
  sources live in the private `embryo-systems-archive`.
- **The grammar leads.** `crates/adt81/grammar.lark` defines ADT'81; the Rust code
  follows it, and a change to the language is made there first, with its
  reason in `docs/adt81.md`.
- **Each engine is a crate of its own, as independent as it can be.** As
  in the print, where each engine (*moteur*) is a project of its own: an
  engine depends on `hbds-core`, the shared vocabulary, and on as few
  engines as it needs, never on a language; a language depends on
  engines, never on another language. `just engines` checks every crate
  against `tools/check_engines.py`, where a new crate states what it may
  use.

## Commands

The flake's dev shell (loaded by direnv) carries every tool. `just` lists the
recipes; `just check` is exactly what CI runs, so a red build reproduces in one
command.

## Conventions

- **A convention arrives with its gate.** If something matters enough to agree
  on, add the check to `just check` in the same change. An unenforced
  convention decays.
- **ASCII in source, with judgement.** No characters confusable with ASCII or
  invisible: em and en dashes, minus signs, smart quotes, middle dots, ellipses,
  non-breaking and zero-width spaces. Visually distinct symbols are fine and
  wanted - degrees, arrows, `>=`. CI enforces exactly this.
- **Docs name only what exists.** Every path and relative link in the
  Markdown files must resolve; `just docs` checks it, so a move or a rename
  updates the docs in the same change.
- **Tool versions live in `flake.nix`**, nowhere else. No version numbers in
  workflow YAML.
- **Renovate defaults are shared** (`nemo/renovate-config`). `renovate.json5`
  only carries this repo's exceptions.
