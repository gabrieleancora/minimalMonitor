# Deployment, upgrades and recovery

This document specifies future operator deliverables. It does not install software, change Nginx/WireGuard, create service units, or provision secrets.

## Existing environment and required inputs

Owner reports 874 MiB total RAM, approximately 574 MB used in htop, existing Nginx, and a WireGuard route to the home network. The available disk budget is strictly 2 GiB. Used/free/available RAM reporting differs, so qualify against the conservative incremental 256 MiB envelope rather than treating the arithmetic remainder as guaranteed headroom.

Before deployment, collect Linux distribution/version, systemd version, CPU, free space on the actual state filesystem, dashboard hostname, certificate arrangement, service port, target CIDRs/ports, DNS path, and backup destination. Inspect only relevant existing Nginx/WireGuard configuration; do not redesign the network. Keep secrets out of handoff reports.

## Development and delivery

- Develop and run ordinary tests natively on Windows. Isolate OS-specific ping, service lifecycle and filesystem permissions behind small interfaces.
- Pin stable Rust and dependency versions at implementation time. Build the production release on Linux CI or WSL2 for `x86_64-unknown-linux-gnu`; determine the oldest supported glibc from the owner's distribution. A musl build is optional only after DNS/TLS/ping compatibility tests.
- Test both Windows and the selected Linux baseline. A successful cross-compile alone does not prove Linux runtime behavior. The [Rust platform-support reference](https://doc.rust-lang.org/rustc/platform-support.html) documents target support; actual deployment compatibility still needs verification.
- Ship a stripped binary containing web assets, checksums, version/build information, dependency/license inventory, operator documentation and later-tested deployment templates. Do not require Rust, Node, compilers, Docker, or a SQLite server on the target.
- Embed/bundle the intended SQLite library and record its actual runtime version for advisory review. TLS trust roots and custom internal CA configuration must be documented and tested on Linux.

## Proposed filesystem layout

| Purpose | Proposed Linux location / access |
| --- | --- |
| Versioned executable | `/opt/minimalmonitor/releases/<version>/`; root-owned, service read/execute |
| Active executable reference | `/opt/minimalmonitor/current`; root-controlled |
| Nonsecret operator configuration | `/etc/minimalmonitor/config.toml`; root-managed, service-readable |
| Telegram token | `/etc/minimalmonitor/secrets/telegram-token`; owner-only or restricted service-group read |
| SQLite and state | `/var/lib/minimalmonitor/`; service writable, private |
| Runtime lock | `/run/minimalmonitor/`; managed at service start |
| Logs | Journal or dedicated rotated output; choose one primary sink |

The future configuration reference must document listen address, public origin, state path, target policy, trust roots, secret-file path, retention and all limits. Invalid production settings fail startup with safe diagnostics. A Windows development profile can use local paths and loopback HTTP, but insecure cookies/bind settings must not leak into production mode.

## Integration sequence

1. Record current host/proxy memory and disk usage; confirm headroom and compatible artifact.
2. Create dedicated service identity and private state directories; securely bootstrap the admin locally.
3. Prove Linux ping permission using the intended group on both configured address families. If this fails, resolve the adapter/host requirement before release; do not quietly remove ping.
4. Start on loopback, verify liveness/readiness, and check HTTP/TCP/ping against explicitly authorized test endpoints.
5. Add one dedicated Nginx virtual host/location with TLS, forwarding, local health-route exclusion and scoped limits. Validate configuration before reload; verify existing sites after reload.
6. Confirm home-network checks use the existing intended routing/DNS, then configure the Telegram secret and chat. Send a test and observe a controlled down/recovery cycle.
7. Complete the acceptance/security/resource gates before declaring internet exposure ready.

## Service management and resource controls

Propose systemd MemoryHigh 96 MiB and MemoryMax 128 MiB for the Rust service; test cgroup accounting and pressure behavior before adopting them. Limits can trigger throttling/OOM and are not performance guarantees. Use restart backoff/start-rate limits, bounded stop time, restricted paths and no new privileges. Leave the existing shared Nginx service limits intact. Review the upstream [systemd execution settings](https://github.com/systemd/systemd/blob/main/man/systemd.exec.xml) and resource controls against the installed version.

Bound aggregate application and monitor-vhost logs to 64 MiB. Avoid changing system-wide journald retention in a way that removes unrelated services' logs; choose per-service storage or a verified namespace where available. Keep a small amount of actionable error metadata and rate-limit repeated failures.

## Backup and restore

Propose daily backup and before upgrades. Keep one local snapshot at most; preflight worst-case snapshot size and available reserve before starting. Use incremental SQLite backup, copy to an existing external destination if available, verify checksum/integrity and permissions, then rotate staging deliberately. If no external destination exists, disclose that the single local backup does not survive host/disk loss.

Back up nonsecret operator config separately; protect the Telegram token through the operator's existing secret backup method. Database backups contain private target/history information. Default recovery objectives are up to 24 hours data loss for a restored daily backup and a documented manual restore in 30 minutes, subject to rehearsal and infrastructure availability.

Restore with the service stopped: preserve the damaged original, restore a verified snapshot into staging, run database integrity and schema compatibility checks, replace state atomically where supported, revoke restored sessions, restart, and confirm new observations and Telegram state. Do not send obsolete pending alerts without applying notification expiry/ordering rules. Verify the recovered instance does not run alongside another scheduler.

## Upgrade and rollback

Keep at most current and previous release artifacts inside the disk budget. Preflight space including migration/backup temporary files, verify checksums, back up, stop service, switch binary, migrate, restart and smoke-test. Publish migration compatibility explicitly.

Binary rollback is safe only if the old binary supports the current database schema. Otherwise stop and restore the matching pre-upgrade database, acknowledging loss of subsequent observations/configuration. Never automatically downgrade schema. Retain a recovery path when readiness fails and record exact rollback evidence.

## Operational checks

The dashboard should expose last scheduler progress, persistence health, retention window, disk pressure, alert backlog/failure and version. Local health checks must not label target outages as process failure. Add an independent external watchdog for the monitor when practical; shared-host or WireGuard failure domains remain visible limitations. Provider DDoS mitigation complements local limits; setting up a new external protection service is outside this planning task.
