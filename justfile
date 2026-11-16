_default:
    @just --list --unsorted

# Compile in debug mode.
build:
    cargo build --workspace

# Build the release binary.
release:
    cargo build --workspace --release

# Run from source.
run *args:
    cargo run -- {{ args }}

# Run the tests.
test:
    cargo test --workspace

# clippy with warnings as errors.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# crates/adt81/grammar.lark: every test program, and random ones, parse one way, to Lark's tree in Rust.
check-grammar:
    cargo build --quiet --workspace --bins --examples
    python3 tools/check_grammar.py

# Every path and link the documentation names exists.
docs:
    python3 tools/check_docs.py

# Each crate depends only on what tools/check_engines.py allows it.
engines:
    python3 tools/check_engines.py

# Format every tracked Rust and Nix file in place.
fmt:
    cargo fmt --all
    nixfmt $(git ls-files '*.nix')

# Fail if anything needs formatting.
fmt-check:
    cargo fmt --all --check
    nixfmt --check $(git ls-files '*.nix')

# The benchmarks, compared against bench/<host>.txt when this host has one.
bench *scenarios:
    cargo run --quiet --release -p embryo-systems-bench -- {{ scenarios }}

# Measure every benchmark and write bench/<host>.txt: a change of speed, on purpose.
bench-record:
    cargo run --quiet --release -p embryo-systems-bench -- --record

# The NixOS module: a system with it enabled gets the service it should.
module:
    python3 tools/check_module.py

# What CI runs, so a red build is reproducible in one command.
check: fmt-check lint test check-grammar engines docs confusables module bench

# Characters that render like ASCII, or render as nothing at all, but compare
# unequal. CI runs this in its own workflow, without a path filter, so it
# covers prose too.

# Grep for characters confusable with ASCII, or invisible.
confusables:
    #!/usr/bin/env bash
    pattern='[\x{2010}-\x{2015}\x{2212}\x{2018}\x{2019}\x{201A}\x{201C}\x{201D}\x{201E}\x{2032}\x{2033}\x{00B7}\x{2026}\x{00D7}\x{00A0}\x{200B}-\x{200D}\x{2060}\x{FEFF}]'
    # 0 is a hit, 1 none; anything else is a grep that did not run, which
    # must not pass for no hit.
    hits=$(git grep -nIP "$pattern")
    case $? in
        0)
            echo "characters confusable with ASCII, or invisible:"
            echo "$hits"
            echo
            echo "Use - . ... x and a plain space instead."
            exit 1
            ;;
        1) ;;
        *)
            echo "the grep for confusable characters failed"
            exit 2
            ;;
    esac

# Remove build output.
clean:
    cargo clean
