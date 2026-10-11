# Production environment and owner inputs

Recorded 2026-10-11 from owner-supplied command output and answers. These are
deployment inputs, not an independent host inspection or application qualification.

## Platform and resource snapshot

| Item | Owner-reported value |
| --- | --- |
| Distribution / architecture | Debian GNU/Linux 12 (bookworm), x86_64 |
| libc | glibc 2.36; Debian package 2.36-9+deb12u14 |
| Kernel | 6.1.0-38-cloud-amd64 |
| Service manager | systemd 252, package 252.39-1~deb12u2; systemd is PID 1; unified hierarchy reported |
| CPU | One virtual CPU, AMD EPYC-Milan; full virtualization, Microsoft hypervisor reported |
| RAM (`free -m`) | 873 MiB total, 726 used, 72 free, 216 buff/cache, 147 available; shared 5 MiB |
| Swap | None |
| Filesystem | /dev/vda1, ext4, mounted at /; both /var/lib and /opt use it |
| Disk (`df -hT`, rounded) | 9.7G total, 6.5G used, 2.9G available, 70% used |
| Mount options | rw,relatime,discard,errors=remount-ro |

Evidence commands: `cat /etc/os-release`, `getconf GNU_LIBC_VERSION`, `uname -m`,
`uname -r`, `systemctl --version`, `ps -p 1 -o comm=`, `ldd --version`, `lscpu`,
`free -m`, `df -hT /var/lib /opt`, and `findmnt -T /var/lib`.

D-14's production baseline is now Debian 12/glibc 2.36. Build elsewhere against
a compatible baseline and verify actual execution there; existing Fedora/glibc
2.43 evidence alone is insufficient. Do not compile on the monitor host or
upgrade its libc to accommodate a development artifact.

The 256 MiB incremental RAM envelope remains a ceiling, not available capacity:
this snapshot has only 147 MiB available. WP-06/WP-07 must remeasure normal-load
headroom and complete-app peak usage, including incremental Nginx overhead,
before deployment. The previous short security exercise does not qualify the
finished service. Preserve the firm 2 GiB incremental disk limit despite the
larger currently reported free space. No swap or host-memory change is assumed.

## Topology, destinations and TLS

- The monitor runs on a VPS; approximately 90% of targets run at home and are
  checked via private endpoints over the existing WireGuard internet link.
- The owner authorizes `192.168.1.0/24` and `10.172.73.1/32`, all TCP ports
  1–65535 and ping in both ranges (D-21). WireGuard AllowedIPs is reported to
  contain these routes; application policy still enforces its own permissions.
  Metadata/special-use denials and subnet network/broadcast exclusions remain.
- A home router/internet/tunnel failure makes affected home endpoints appear
  unreachable from the VPS. Preserve per-monitor incidents and the existing
  Telegram digest; do not claim to diagnose the shared root cause automatically.
- Public dashboard origin: `https://monitor.gabry.win`, behind existing Nginx
  and an owner-managed wildcard certificate. WP-06 verifies coverage and renewal.
- Private hostnames resolve on the VPS. Internal HTTPS uses Let's Encrypt
  certificates for `*.local.gabry.win`; preserve each target's hostname/SNI while
  connecting to validated private addresses, with ordinary certificate checking.
  No custom CA is currently requested. Exact service hostnames/ports are still
  monitor configuration inputs, not additional permission decisions.
- Backend listen port, installed Nginx version/integration details, DNS resolver
  path and narrow production ping-group permissions remain WP-06 checks.

## Alerts and backups

Telegram delivery is to one channel containing the owner and other bots. WP-04/
WP-06 must provision the channel ID and secret-file token, grant the bot channel
posting permission, and verify a test message. No inbound bot functionality is
requested. See the [notification contract](../design/notifications.md).

The sending Telegram bot is shared with other owner applications (D-22), so
local pacing cannot prevent all provider rate limits. The owner also operates
a Bark server at home for iPhone notifications. Its optional [v2 fallback](../design/bark-fallback.md)
does not replace Telegram and may be unreachable during the same home outage.
Its endpoint/version and secret provisioning remain future integration inputs.

The owner is considering a NAS-initiated SSH pull from target servers. This is
a preferred direction, not a configured backup system. WP-06 should prepare a
consistent SQLite snapshot through the service's supported backup mechanism,
then let the NAS fetch that completed snapshot. Do not copy a live database/WAL
piecemeal or bypass the shared ownership lock with a competing database command.
NAS destination, restricted SSH identity, schedule, remote retention and restore
rehearsal remain to be settled. The existing one-local-snapshot disk allowance
still applies; a home outage can delay NAS collection while the VPS remains up.

## Current readiness

Owner choices resolve the production platform, check cadence, private range/
port/ping permissions, topology, dashboard origin, internal TLS and Telegram
destination type. D-19 defines faster confirmation checks; D-21 requires a
bounded all-TCP grant representation beyond the completed WP-02 interface.
These are next-package contracts, not completed code or deployment approval.
