"""Read-only WP-00 development-host diagnostic; no permission changes."""
import os
import shutil
import socket

print(f"uid={os.getuid()} gid={os.getgid()} groups={os.getgroups()}")
for tool in ("cargo", "rustc", "cc", "gcc", "clang", "python3", "curl"):
    print(f"tool_{tool}={shutil.which(tool)}")
with open("/proc/sys/net/ipv4/ping_group_range", encoding="ascii") as f:
    print(f"ping_group_range={f.read().strip()}")
for family, protocol, label in (
    (socket.AF_INET, socket.IPPROTO_ICMP, "ipv4"),
    (socket.AF_INET6, socket.IPPROTO_ICMPV6, "ipv6"),
):
    try:
        with socket.socket(family, socket.SOCK_DGRAM, protocol):
            print(f"{label}_datagram_socket=opened (echo not tested)")
    except OSError as error:
        print(f"{label}_datagram_socket=errno:{error.errno} {error.strerror}")
