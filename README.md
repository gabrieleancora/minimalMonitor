# minimalMonitor

WP-01–WP-05 implement the Rust/SQLite backend and authenticated browser GUI:
HTTP/HTTPS/TCP/native ping, incidents, coverage, bounded Telegram delivery,
responsive monitor management, history, account settings and light/dark/system
appearance. This is not a production release. See [the plan](plan/README.md),
[WP-05 handoff](plan/agents/reports/WP-05.md) and
[desktop/mobile review evidence](validation/wp05/README.md).

## Build and validate

`rust-toolchain.toml` pins Rust 1.98.1; `Cargo.lock` pins the application graph.
Bundled SQLite needs a C compiler on the development builder, not on the service
host. The production baseline is Debian 12 x86_64/glibc 2.36; Fedora development
builds still require separate Debian-compatible artifact validation. See the
[production environment record](plan/operations/production-environment.md).

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python validation/wp04/lifecycle.py target/release/minimalmonitor.exe
```

Use `target/release/minimalmonitor` and `python3` on Linux. For the documented
remote development machine, run `bash validation/wp04/run-linux.sh` from the
isolated `validation/wp04/workspace` checkout under the approved project root.
The runner records evidence and uses its existing stable
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

`python validation/wp03/dependency_review.py` records licenses from Cargo metadata
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
| `extra_trust_roots` | Optional DER certificate paths relative to the config file; at most 16 files, 16 KiB each, 128 KiB total. Public trust roots remain enabled. |

Unknown fields are rejected. No environment proxy, secrets or runtime bounds are
silently inherited. Telegram secret-file configuration belongs to WP-04.
Checks use validated numeric addresses and preserve the TLS hostname without a
second resolution. No Telegram sender exists yet.

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
database-worker round trip/write admission and recent scheduler progress. Target
failures do not make the monitor process unready.

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
`cidr`; it may narrow `host`, list `ports` or set `all_tcp_ports=true`, and set
`ping=true` independently. The all-TCP flag requires an empty/omitted port list
and grants ports 1–65535. At most 64 rules
and 64 ports per rule are accepted. Private destinations require a matching rule
for the protocol/port and optional hostname. Loopback additionally requires /32
or /128. Hard-denied address classes cannot be overridden. No browser endpoint
can change these rules. Public unicast is allowed. URL queries may be monitored
but never formatted into diagnostics. The [policy module](src/destination.rs)
rejects mixed DNS answers and returns at most 32 validated numeric addresses;
monitoring transports still reject whole DNS resolutions exceeding eight
answers, without truncation. AWS IPv6 metadata `fd00:ec2::254` is unconditionally denied
even under a private destination grant.

## Bounds and persistence handoff

The runtime has two async workers, six maximum blocking helpers, one dedicated
SQLite thread and 128 queued database commands. Database admission uses
`try_send`; command execution and response waits have independent deadlines.
Excess work returns an explicit safe error. Query progress cancellation and a
two-second SQLite busy timeout keep expensive/contended work off async workers.
Shared nonwaiting permits reserve four probes, one password hash and one alert
sender. WP-02 holds the hash permit on a blocking task through completion even
after request cancellation. At most four uncancellable native DNS/ICMP calls
retain probe permits through completion; a fifth helper can hash and a sixth
can join storage during shutdown. Alert sending remains WP-04.

HTTP/1 accepts at most 16 active connections, including slow header readers and
response writers. It closes after one request, enforces 16 KiB header and body
budgets plus a five-second connection deadline, and closes excess connections.
Monitoring API responses also enforce the 256 KiB response limit. Health
routes accept no query or request body. Normal application errors have a stable
code, safe message and generated request ID. Malformed HTTP rejected by Hyper
can use its protocol response rather than an application error envelope.

[Migration 1](migrations/0001_foundation.sql) defines the planned entities and
indexes with no cascading history deletion, singleton records, FK constraints,
unique observation slots/logical incident events, and session/monitor caps.
[Migration 2](migrations/0002_monitoring.sql) adds persisted cadence, next due,
system-fault categories and schedule generations. Results, hourly counts,
state, incidents and compact notification transitions commit together. Confirmed
180-day incident retention is unchanged; the owner
removed the proposed incident row-count cap in [D-18](plan/decisions.md#d-18-incident-retention-without-a-row-count-cap-confirmed-2026-10-11).
Existing database/disk-pressure safeguards apply; the overflow planning prerequisite is resolved.

SQLite uses WAL, FULL synchronization, FK enforcement, a 2 MiB cache target,
defensive mode, bounded SQL/value/column/parameter sizes and no attached
databases. Its main page ceiling is at most 512 MiB for the actual page size.
Readiness checks filesystem space, bounded state-directory accounting and WAL
pressure; 32 MiB is a write-pressure threshold, not a hard WAL cap. Maintenance
tries one SQLite-managed TRUNCATE checkpoint without waiting for locks when
PASSIVE leaves the WAL at that threshold, then remeasures admission. Pinned
transactions keep admission blocked until a later check can reclaim the WAL;
recovery requires no restart. All monitoring mutations check admission
**inside the database worker** before writing.
Below 128 MiB free, admission fails; below 256 MiB or above 400 MiB main database,
the internal health snapshot warns. Unsaved results remain unknown coverage.

## Monitoring backend

Use the authenticated `/api/v1/monitors` JSON API; browser forms remain WP-05.
Create fields/defaults follow [the monitoring contract](plan/design/monitoring.md).
Provide `Idempotency-Key` when retrying creation. Edits contain `{revision, config}`;
pause/resume contain `{revision}`; deletion additionally requires
`confirm_history_deletion=true`. Every mutation requires Origin/session/CSRF.
Summary exposes pending counts, next due, system faults and dropped alerts.
Monitor IDs are opaque signed integers: existing positive IDs are retained,
while migration 3 allocates new IDs from a durable negative sequence. Always use
the returned ID. Cleanup cannot recycle an ID for a replacement, and retained
create keys for deleted monitors return a conflict.
History accepts `start_ms`/`end_ms`; incidents accept `before`/`limit` (maximum 100).

Four slots check enabled monitors independently of browsers. Pending failures
replace the normal schedule with completion plus 30 seconds. Edits, pause and
restart discard old work; gaps reset streaks and preserve unresolved incidents
with a gap flag. Recent history uses exact scheduled-slot counts; older history
sums hourly buckets and labels partial boundaries. Availability is sample-based,
with separate coverage and N/A for empty denominators.
Cleanup deletes at most 500 history rows per command: seven-day raw, 90-day
hourly and 180-day closed incidents; open incidents survive. Disk/WAL limits
can reject writes without shortening incident retention.
Restart and clock-jump catch-up run bounded eligible cleanup before each attempt,
so history row limits cannot prevent their own reclamation. Non-reclaimable
capacity and disk/WAL pressure remain visibly degraded.

Startup rejects foreign, inconsistent or newer schema versions and fails before
binding. Migration DDL, data and version records commit together. Future upgrades
preflight worst-case main/WAL growth, one supported SQLite backup and the firm
2 GiB allocation. Reserve the full 512 MiB snapshot ceiling because committed
pages can remain in WAL; count WAL/shared-memory metadata together. An existing
`pre-migration.db` blocks replacement and requires
operator review. There is no schema downgrade, automatic snapshot deletion or
full VACUUM. Backup/restore deployment workflows and full resource qualification
remain future package work.

## Telegram backend (WP-04)

Provision the bot token in a service-readable secret file **outside this
repository**. Add optional `telegram_token_file` to TOML; relative paths resolve
against the configuration directory. Linux requires a regular nonsymlink 0600
file; Windows operators must restrict its ACL to the service/owner account.
The file is at most 256 bytes and contains only the bot token (one trailing line
ending is allowed). Never paste a token into browser JSON, command arguments,
logs or this repository. `check-config` validates the file without creating state.
Restart to load a changed secret; credential changes cancel queued destinations
with an audit record and preserve provider cooldowns.

With an authenticated session/Origin/CSRF, PATCH `/api/v1/settings/telegram` with
`{revision,enabled,chat_id}`. Use a canonical signed decimal string for the chat
ID; the backend also accepts an exact integer. GET returns settings, token
presence and delivery health, never the token. Enabling requires a token and
nonzero chat ID. The empty-body test POST returns a durable notification ID with
202 and is limited to once a minute. Paginate `/api/v1/notifications` using
`before`/`limit` (maximum 100). GUI forms and banners belong to WP-05.

One independent sender packs dated down/recovery events after five seconds,
paces messages at three seconds, and retries transient failures after 60 seconds
from completion. A shared-bot 429 cooldown applies to every local send across
restart. Permanent credential/chat failures stop requests until a corrective
settings update or changed secret. Pending events expire at six hours or ten
admitted attempts; a crash during an admitted request may leave its delivery
uncertain and count that reserved attempt. Stable incident IDs identify possible
duplicates. An already submitted old-destination request may finish after settings
change; queued jobs are canceled and never redirected.

The sender uses only the official Telegram HTTPS origin, public TLS roots, safe
error categories and bounded response parsing. It ignores proxy environment
variables and follows no redirects. Delivery failure leaves checks operational;
summary exposes backlog, delays, permanent failures, missing token, suppression
and acknowledgement-storage degradation. This scoped implementation does not
establish live channel posting permissions, Debian compatibility, release resource
footprint or production integration.

If a claim commits but its reply is lost before transport starts, the running
sender retains admission and reports degraded delivery health until an idempotent
SQLite reconciliation confirms recovery. Unsent reservations are refunded;
expiry, cancellation and provider waits are preserved. This does not resend a
request with a known transport outcome awaiting acknowledgement.
