#!/usr/bin/env bash
set -eu
project=$(pwd -P)
case "$project" in
 /home/gabriele/rust-develop/minimalMonitor/validation/wp05/workspace|/home/gabriele/rust-develop/minimalMonitor/validation/wp05/corrections-20261018) ;;
 *) exit 1 ;;
esac
export RUSTUP_TOOLCHAIN=stable RUSTUP_AUTO_INSTALL=0
export CARGO_HOME=/home/gabriele/rust-develop/minimalMonitor/validation/wp03/workspace/.cargo
export CARGO_TARGET_DIR="$project/target" TMPDIR="$project/.tmp"
evidence="$project/validation/wp05/evidence/fedora"
mkdir -p "$evidence" "$TMPDIR"
{ date -u +%FT%TZ; uname -a; cat /etc/os-release; id; rustc -Vv; cargo -V; ldd --version | head -n 1; } > "$evidence/environment.txt" 2>&1
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
run_check clippy cargo clippy --locked --offline --all-targets -- -D warnings
run_check test cargo test --locked --offline
run_check release cargo build --release --locked --offline
run_check version "$CARGO_TARGET_DIR/release/minimalmonitor" --version
run_check lifecycle python3 validation/wp04/lifecycle.py "$CARGO_TARGET_DIR/release/minimalmonitor"
sha256sum "$CARGO_TARGET_DIR/release/minimalmonitor" > "$evidence/artifact.txt"
stat -c '%s bytes' "$CARGO_TARGET_DIR/release/minimalmonitor" >> "$evidence/artifact.txt"
