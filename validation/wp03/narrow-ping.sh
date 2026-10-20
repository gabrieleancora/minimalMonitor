#!/usr/bin/env bash
# No host-wide policy change: verify namespace separation before any write.
set -eu
project=$(pwd -P)
test "$project" = /home/gabriele/rust-develop/minimalMonitor/validation/wp03/workspace
test_binary=$(find target/debug/deps -maxdepth 1 -type f -executable -name 'probe_transports-*' | head -n 1)
test -n "$test_binary"
export WP03_PING_BINARY="$project/$test_binary"
export WP03_HOST_NAMESPACE=$(readlink /proc/self/ns/net)
unshare --user --map-current-user --keep-caps --net bash -eu -c '
    test "$(readlink /proc/self/ns/net)" != "$WP03_HOST_NAMESPACE"
    test "$(id -u)" != 0
    test "$(id -g)" != 0
    ip link set lo up
    cat /proc/sys/net/ipv4/ping_group_range
    setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
      "$WP03_PING_BINARY" --exact native_ping_permission_is_a_local_fault --ignored
    printf "%s %s\n" "$(id -g)" "$(id -g)" > /proc/sys/net/ipv4/ping_group_range
    setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
      bash -eu -c '\''
        id
        cat /proc/sys/net/ipv4/ping_group_range
        sed -n "/^Cap/p; /^NoNewPrivs/p" /proc/self/status
        "$WP03_PING_BINARY" --exact native_ping_v4_v6_and_system_dns_are_real_platform_checks
      '\''
'
printf 'host_ping_group_range_after='
cat /proc/sys/net/ipv4/ping_group_range
