#!/usr/bin/env bash
# Run from the approved remote project root. Use the existing toolchain without
# downloading a new shared rustup toolchain; record its exact version below.
set -eu
project=$(pwd -P)
test "$project" = /home/gabriele/rust-develop/minimalMonitor
export RUSTUP_TOOLCHAIN=stable
export CARGO_HOME="$project/.cargo"
export CARGO_TARGET_DIR="$project/target"
export TMPDIR="$project/.tmp"
mkdir -p "$CARGO_HOME" "$TMPDIR" "$project/validation/wp01/evidence/fedora"
evidence="$project/validation/wp01/evidence/fedora"
{
    date -u +%FT%TZ
    uname -a
    cat /etc/os-release
    id
    rustc -Vv
    cargo -V
    ldd --version | head -n 1
} > "$evidence/environment.txt" 2>&1
: > "$evidence/exits.tsv"
run_check() {
    local name=$1
    shift
    local status=0
    "$@" > "$evidence/$name.log" 2>&1 || status=$?
    printf '%s\t%s\n' "$name" "$status" >> "$evidence/exits.tsv"
    printf '%s exit=%s\n' "$name" "$status"
    if [ "$status" -ne 0 ]; then tail -n 50 "$evidence/$name.log"; exit "$status"; fi
}
run_check fmt cargo fmt --all -- --check
run_check clippy cargo clippy --locked --all-targets -- -D warnings
run_check test cargo test --locked
run_check release cargo build --release --locked
run_check version "$CARGO_TARGET_DIR/release/minimalmonitor" --version
run_check lifecycle python3 validation/wp01/lifecycle.py "$CARGO_TARGET_DIR/release/minimalmonitor"
sha256sum "$CARGO_TARGET_DIR/release/minimalmonitor" > "$evidence/artifact.txt"
stat -c '%s bytes' "$CARGO_TARGET_DIR/release/minimalmonitor" >> "$evidence/artifact.txt"
file "$CARGO_TARGET_DIR/release/minimalmonitor" >> "$evidence/artifact.txt"
ldd "$CARGO_TARGET_DIR/release/minimalmonitor" >> "$evidence/artifact.txt"
