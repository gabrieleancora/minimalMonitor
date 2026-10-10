# minimalMonitor

WP-01 foundation and WP-02 security backend for a small Rust/SQLite monitor.
This intermediate build provides local account commands, a minimal login form,
cookie sessions, CSRF, protected account endpoints and destination validation.
Monitoring, Telegram and the dashboard GUI remain separate packages.
Authenticated requests to pending feature routes return a safe 501; no monitor
status is fabricated. This is not a production release. See
[the plan](plan/README.md) and [WP-02 evidence](plan/agents/reports/WP-02.md).

## Build and validate

`rust-toolchain.toml` pins Rust 1.98.1; `Cargo.lock` pins the application graph.
Bundled SQLite needs a C compiler on the development builder, not on the service
host. The production distribution/minimum glibc remains an owner decision.

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python validation/wp02/lifecycle.py target/release/minimalmonitor.exe
```

Use `target/release/minimalmonitor` and `python3` on Linux. For the documented
remote development machine, run `bash validation/wp02/run-linux.sh` from the
approved project root. The runner records evidence and uses its existing stable
toolchain; it installs no shared tooling. The Windows session also used the
existing stable alias (Rust 1.98.1), because no separately named 1.98.1 toolchain
was installed. To use that existing installation without downloading another:

```powershell
$env:RUSTUP_TOOLCHAIN = 'stable'
$env:RUSTUP_AUTO_INSTALL = '0'
$env:CARGO_HOME = Join-Path (Get-Location) '.cargo'
```

The explicit alias override may select a different compiler on another machine;
record `rustc -Vv` when using it. Prefer the pinned toolchain in ordinary builds.
Build products, Cargo downloads, temporary state and raw validation evidence are
development artifacts and are excluded from shipment.

`python validation/wp02/dependency_review.py` records licenses from Cargo metadata
and checks every registry package/version in the lockfile against OSV. This
requires network access and is a known-advisory check, not a security proof.

## Foundation commands and configuration

```text
minimalmonitor --version
minimalmonitor check-config config/development.toml
minimalmonitor serve config/development.toml
minimalmonitor admin-bootstrap config/development.toml
minimalmonitor admin-reset config/development.toml
```

`check-config` reads and validates at most 16 KiB, emits no configuration values,
and creates no state. `serve` creates the state directory, takes exclusive
ownership, opens SQLite on one dedicated thread, validates/migrates it, then binds
the listener. Ctrl+C and Linux SIGTERM stop admission, drain bounded work and
checkpoint/close SQLite. Startup errors return exit 1 with a safe category.

| Field | Foundation contract |
| --- | --- |
| `mode` | Required `development` or `production` |
| `listen` | Required loopback IPv4/IPv6 address and nonzero port in both modes |
| `state_dir` | Required path; relative paths resolve against the config file's directory |
| `public_origin` | Required origin without credentials, path other than `/`, query or fragment; HTTPS in production, loopback HTTP additionally allowed in development |
| `trusted_proxy` | Optional loopback IP; only this socket peer's single numeric `X-Real-IP` is accepted. Nginx must overwrite it. Omit to ignore identity headers. |
| `destination_rules` | Optional operator-only private destination grants; see below |

Unknown fields are rejected. No environment proxy, secrets or runtime bounds are
silently inherited. Telegram secret-file configuration belongs to WP-04.
This build performs no outbound checks or sends; WP-03 must consume validated
numeric addresses and preserve the TLS hostname without resolving again.

On Linux, state directories must be private (0700) and existing state files must
exclude group/other access. Newly created lock/database/snapshot files use 0600;
SQLite derives sidecar permissions from the database. On Windows, files inherit
the operator-managed directory ACL; validation does not claim to audit Windows
ACLs. Keep the state directory on a local filesystem with working OS locks and
restrict its parent directory. Symlinks at state/file paths are refused. Never
remove `owner.lock` while the service or a local account command is running.

Local health requests use the accepted socket identity and ignore forwarded
identity headers. Nginx must exclude both health paths when integrated in WP-06:
a local reverse proxy is itself a local socket peer. Readiness confirms a fresh
database-worker round trip and write admission, not target status or future
workers that have not been implemented.

## Account and destination security

Stop the service before bootstrap/reset. Both commands take the same persistent
ownership lock before prompting, require an interactive terminal, hide typed
credentials, confirm the new password and accept no credential arguments or
environment variables. Bootstrap refuses an existing account; reset refuses an
absent account. Reset and password changes atomically advance the credential
generation and revoke every session. State files contain password hashes and
operational data and must remain private.

Visit `/login` at the configured origin. The form uses a signed five-minute
preauth cookie/token and a local script to submit JSON with `X-CSRF-Token`.
Successful login issues a fresh opaque session and returns its CSRF binding.
`GET /api/v1/account/session` returns only the current CSRF token.
`POST /auth/logout` and `POST /api/v1/account/password` require the session,
canonical Origin and CSRF header. Password change JSON contains
`current_password` and `new_password`. Credential JSON is capped at 1024 bytes;
other bodies at 16 KiB. There is no public registration or setup endpoint.

Production uses Secure/HttpOnly/SameSite=Lax `__Host-` cookies with Path=/ and no
Domain. Explicit loopback HTTP development uses separate `mm-dev-*` cookie names.
Every non-health request must supply the canonical Host; mutations additionally
require the canonical Origin. Nginx must forward the canonical Host, and overwrite
`X-Real-IP` if `trusted_proxy` is set. Without a trusted proxy, limits group
requests by accepted socket IP. See the authoritative
[security contract](plan/operations/security.md) for password parameters, session
expiry, admission and proxy responsibilities.

Operator grants are optional TOML arrays. Each `[[destination_rules]]` requires
`cidr`; it may narrow `host`, list `ports`, and set `ping=true`. At most 64 rules
and 64 ports per rule are accepted. Private destinations require a matching rule
for the protocol/port and optional hostname. Loopback additionally requires /32
or /128. Hard-denied address classes cannot be overridden. No browser endpoint
can change these rules. Public unicast is allowed. URL queries may be monitored
but never formatted into diagnostics. The [policy module](src/destination.rs)
rejects mixed DNS answers and returns at most 32 validated numeric addresses;
monitoring transports still reject whole DNS resolutions exceeding eight
answers, without truncation. WP-03 owns bounded resolution, transports, TLS and
redirect suppression. AWS IPv6 metadata `fd00:ec2::254` is unconditionally denied
even under a private destination grant.

## Bounds and persistence handoff

The runtime has two async workers, two maximum blocking helpers, one dedicated
SQLite thread and 128 queued database commands. Database admission uses
`try_send`; command execution and response waits have independent deadlines.
Excess work returns an explicit safe error. Query progress cancellation and a
two-second SQLite busy timeout keep expensive/contended work off async workers.
Shared nonwaiting permits reserve four probes, one password hash and one alert
sender. WP-02 holds the hash permit on a blocking task through completion even
after request cancellation. Probe and alert workers remain later packages.

HTTP/1 accepts at most 16 active connections, including slow header readers and
response writers. It closes after one request, enforces 16 KiB header and body
budgets plus a five-second connection deadline, and closes excess connections.
Fixed health/error/login/account bodies remain below the 256 KiB response limit. Health
routes accept no query or request body. Normal application errors have a stable
code, safe message and generated request ID. Malformed HTTP rejected by Hyper
can use its protocol response rather than an application error envelope.

[Migration 1](migrations/0001_foundation.sql) defines the planned entities and
indexes with no cascading history deletion, singleton records, FK constraints,
unique observation slots/logical incident events, and session/monitor caps.
Feature workers must still enforce validation, revision checks, bounded queues,
retention, outbox limits, and atomic transitions; tables alone do not pass their
acceptance gates. Confirmed 180-day incident retention is unchanged; WP-03 must
resolve the closed-incident overflow policy before implementing writes.

SQLite uses WAL, FULL synchronization, FK enforcement, a 2 MiB cache target,
defensive mode, bounded SQL/value/column/parameter sizes and no attached
databases. Its main page ceiling is at most 512 MiB for the actual page size.
Readiness checks filesystem space, bounded state-directory accounting and WAL
pressure; 32 MiB is a write-pressure threshold, not a hard WAL cap. Maintenance
tries one SQLite-managed TRUNCATE checkpoint without waiting for locks when
PASSIVE leaves the WAL at that threshold, then remeasures admission. Pinned
transactions keep admission blocked until a later check can reclaim the WAL;
recovery requires no restart. Later mutation
commands must check admission **inside the database worker** before writing.
Below 128 MiB free, admission fails; below 256 MiB or above 400 MiB main database,
the internal health snapshot warns. No history or configuration is acknowledged
as saved by this foundation.

Startup rejects foreign, inconsistent or newer schema versions and fails before
binding. Migration DDL, data and version records commit together. Future upgrades
preflight worst-case main/WAL growth, one supported SQLite backup and the firm
2 GiB allocation. Reserve the full 512 MiB snapshot ceiling because committed
pages can remain in WAL; count WAL/shared-memory metadata together. An existing
`pre-migration.db` blocks replacement and requires
operator review. There is no schema downgrade, automatic snapshot deletion or
full VACUUM. Backup/restore deployment workflows and full resource qualification
remain future package work.
