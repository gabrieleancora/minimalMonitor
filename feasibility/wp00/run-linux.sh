#!/usr/bin/env bash
# Run from the remote project's approved root. All writable Cargo/temp/log paths
# stay below this root; the existing host toolchain is only read.
set -u
project=$(pwd -P)
prototype="$project/feasibility/wp00"
test -f "$prototype/Cargo.lock" || exit 2
export CARGO_HOME="$project/.cargo"
export CARGO_TARGET_DIR="$prototype/target"
export TMPDIR="$project/.tmp"
mkdir -p "$CARGO_HOME" "$TMPDIR" "$prototype/evidence/fedora"
evidence="$prototype/evidence/fedora"
cd "$prototype" || exit 2

{
    date -u +%FT%TZ
    uname -a
    cat /etc/os-release
    id
    rustc -Vv
    cargo -V
    cc --version | head -n 1
    ldd --version | head -n 1
    rustup show active-toolchain
    cat /proc/sys/net/ipv4/ping_group_range
    cat /proc/self/cgroup
    free -b
    df -T "$project"
    lscpu
    python3 linux-environment.py
} > "$evidence/environment.txt" 2>&1

: > "$evidence/exits.tsv"
: > "$evidence/commands.txt"
run_check() {
    local name=$1
    shift
    printf '%q ' "$@" >> "$evidence/commands.txt"
    printf '\n' >> "$evidence/commands.txt"
    "$@" > "$evidence/$name.log" 2>&1
    local status=$?
    printf '%s\t%s\n' "$name" "$status" >> "$evidence/exits.tsv"
    printf '%s exit=%s\n' "$name" "$status"
    if [ "$status" -ne 0 ]; then tail -n 40 "$evidence/$name.log"; fi
}

run_check fmt cargo fmt --all -- --check
run_check clippy cargo clippy --locked --all-targets -- -D warnings
run_check test cargo test --locked
run_check release cargo build --release --locked
if [ -x "$CARGO_TARGET_DIR/release/minimalmonitor-wp00" ]; then
    run_check measure /usr/bin/time -v -o "$evidence/time.txt" "$CARGO_TARGET_DIR/release/minimalmonitor-wp00"
    stat -c '%n %s bytes' "$CARGO_TARGET_DIR/release/minimalmonitor-wp00" > "$evidence/files.txt"
    sha256sum "$CARGO_TARGET_DIR/release/minimalmonitor-wp00" >> "$evidence/files.txt"
    du -sb "$CARGO_TARGET_DIR" "$CARGO_HOME" "$prototype/target/wp00-state" >> "$evidence/files.txt"
    file "$CARGO_TARGET_DIR/release/minimalmonitor-wp00" >> "$evidence/files.txt"
    ldd "$CARGO_TARGET_DIR/release/minimalmonitor-wp00" >> "$evidence/files.txt"
fi
cat "$evidence/exits.tsv"
if awk '$2 != 0 { failed=1 } END { exit failed }' "$evidence/exits.tsv"; then
    exit 0
else
    exit 1
fi
