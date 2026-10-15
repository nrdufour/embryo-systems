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

# What CI runs, so a red build is reproducible in one command.
check: fmt-check lint test check-grammar engines confusables

# Characters that render like ASCII, or render as nothing at all, but compare
# unequal. CI runs this in its own workflow, without a path filter, so it
# covers prose too.

# Grep for characters confusable with ASCII, or invisible.
confusables:
    #!/usr/bin/env bash
    pattern='[\x{2010}-\x{2015}\x{2212}\x{2018}\x{2019}\x{201A}\x{201C}\x{201D}\x{201E}\x{2032}\x{2033}\x{00B7}\x{2026}\x{00D7}\x{00A0}\x{200B}-\x{200D}\x{2060}\x{FEFF}]'
    if hits=$(git ls-files | xargs grep -nIP "$pattern" 2>/dev/null); then
        echo "characters confusable with ASCII, or invisible:"
        echo "$hits"
        echo
        echo "Use - . ... x and a plain space instead."
        exit 1
    fi

# Remove build output.
clean:
    cargo clean
