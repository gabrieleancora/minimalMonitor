#!/usr/bin/env bash
# Development test only: never change ping permission in the host namespace.
set -eu
project=$(pwd -P)
export CARGO_HOME="$project/.cargo"
export CARGO_TARGET_DIR="$project/feasibility/wp00/target"
export TMPDIR="$project/.tmp"
prototype="$project/feasibility/wp00"
mkdir -p "$prototype/evidence/fedora"
host_namespace=$(readlink /proc/self/ns/net)
export WP00_HOST_NAMESPACE="$host_namespace"
export WP00_PROTOTYPE="$prototype"

unshare --user --map-current-user --keep-caps --net bash -eu -c '
    test "$(readlink /proc/self/ns/net)" != "$WP00_HOST_NAMESPACE"
    test "$(id -u)" != 0
    test "$(id -g)" != 0
    ip link set lo up
    cd "$WP00_PROTOTYPE"
    printf "isolated_namespace=%s\n" "$(readlink /proc/self/ns/net)"
    # Read the namespace default first; unmapped GIDs cannot be written as a
    # denied range from this one-user mapping. No host sysctl is touched.
    printf "namespace_default_ping_group_range="
    cat /proc/sys/net/ipv4/ping_group_range
    setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
      python3 linux-environment.py
    # This write affects only the disposable network namespace and admits one GID.
    printf "%s %s\n" "$(id -g)" "$(id -g)" > /proc/sys/net/ipv4/ping_group_range
    setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
      bash -eu -c '\''
        id
        cat /proc/sys/net/ipv4/ping_group_range
        sed -n "/^Cap/p; /^NoNewPrivs/p" /proc/self/status
        cargo test --locked --offline ping::tests::local_ipv4_ipv6_echo -- --exact
      '\''
'
printf "host_ping_group_range_after="
cat /proc/sys/net/ipv4/ping_group_range
