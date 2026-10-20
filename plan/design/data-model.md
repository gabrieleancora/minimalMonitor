# Data model and persistence

This document remains the logical contract. WP-01 adds [foundation migration 1](../../migrations/0001_foundation.sql) outside `plan`; its [report](../agents/reports/WP-01.md) records executable schema and migration evidence. WP-02 implements transactional account/session mutations and bounded security audit metadata on the same worker, without changing the schema; see its [report](../agents/reports/WP-02.md) and [security contract](../operations/security.md). WP-03 adds [migration 2](../../migrations/0002_monitoring.sql), cadence/state/coverage, bounded retention and incident/outbox transitions. Telegram delivery remains WP-04. Use UTC timestamps, monotonic clocks for in-process elapsed time, parameterized SQL, explicit constraints and versioned migrations.

WP-04 adds [migration 4](../../migrations/0004_notifications.sql) and the bounded
`telegram_state` singleton: credential fingerprint (never the token), bot-wide
cooldown, destination pacing, test-action limit, permanent-error latch, request/
acknowledgement sequence and suppression cursor. Outbox terminal timestamps and
safe error categories support seven days after completion and at most 2000
terminal receipts. A receipt bit on each incident remembers delivered down
alerts after receipt cleanup; long incidents cannot acquire false nondelivery
claims on recovery. A credential change/removal/provisioning cancels prior queued
destinations, advances revisions and audits the change without clearing cooldown.
Migration uses the existing supported snapshot/preflight rules.

## Entities

| Entity | Key fields / constraints |
| --- | --- |
| `schema_versions` | Applied migration identifier and timestamp |
| `admin` | Single row; normalized username, Argon2id password hash, credential generation, timestamps |
| `sessions` | Hash of opaque random token, admin ID, CSRF token binding, created/last-used/absolute-expiry times; max five active sessions |
| `monitors` | ID, name, type, validated target fields, interval/deadline, failure threshold, enabled/alerts flags, address family, revision |
| `monitor_identity_allocator` | Migration 3 singleton; durable decreasing sequence for newly allocated negative monitor IDs; legacy positive IDs remain unchanged |
| `schedule_epochs` | Monitor/revision, anchor, normal interval, cadence mode/next due slot, enabled start/end, coverage discontinuities; enough data to reconstruct expected normal and confirmation slots |
| `monitor_state` | One row per monitor: status, failure streak, last due/completed time, last result/error, active incident ID |
| `probe_results` | Monitor/revision, scheduled slot, observed time, outcome, elapsed milliseconds, status code, bounded error category; unique monitor/revision/slot |
| `hourly_stats` | Monitor/hour: expected, successful, failed, unknown slot counts; latency sum/count/max; unique monitor/hour |
| `incidents` | Monitor/revision, first failure time, confirmation time, end time/reason, last evidence, observation-gap flag |
| `notification_outbox` | Event identity, incident, kind, destination revision, compact payload fields, state, attempts, next attempt, creation/expiry, message ID |
| `settings` | Nonsecret global settings, Telegram chat/destination revision, configured public origin |
| `request_keys` | Admin-scoped idempotency key, operation, request digest and committed result reference; at most 100 rows for 24 hours |
| `health_state` | Bounded singleton operational state and counters, including dropped alerts and persistence faults |
| `audit_events` | Bounded security/configuration event metadata; no tokens/passwords or probe payloads |

WP-04 must persist Telegram's bot-wide rate-limit not-before timestamp in bounded singleton state, as well as each event's retry/expiry state; restarts must not bypass a provider cooldown (D-22). Optional v2 Bark provider outcomes and incident recovery routing are described in the [future fallback contract](bark-fallback.md); no Bark schema or generalized provider framework is required for WP-04.

WP-04 implements this singleton and preserves it on restart. Claim reserves one
durable request sequence and bounded event/attempt references; Finish is
idempotent for that sequence, including canceled/pruned in-flight jobs. A canceled
old destination's 429 still gates the bot, while its permanent chat failure cannot
disable a replacement destination. Interrupted sending jobs return to pending
with a 60-second restart wait, preserving expiry and attempt bounds. Actual send
outcomes acknowledge the durable sequence once. A settings change cannot retract
an already submitted request; its captured destination never changes mid-flight.

For WP04-R1, one in-memory permit records an uncertain **unsubmitted** claim.
A worker-ordered reconciliation transaction refunds the unsent reservation only
on still-sending rows, expires rows at their original deadline, and acknowledges
the outstanding sequence. It never revives canceled/terminal jobs or resets
cooldown/pacing, destination revisions, due times or incident ordering. The
sequence acknowledgement makes repeated reconciliation idempotent after a lost
reply. A lost claim/reconciliation reply keeps admission and degraded health
until the same sender receives confirmation; no process restart or new schema is
needed. This operation is never applied to a transport outcome awaiting Finish.

Foreign keys apply consistently. Index probe results by monitor/time, incidents by monitor/confirmation time, sessions by token hash and expiry, and pending notifications by state/next-attempt time. Delete monitor-owned history in bounded batches after disabling its schedule and canceling queued notifications; hide the tombstoned monitor immediately. No uncontrolled cascade through years of data in an interactive request.

An idempotency key reused with a different request digest is a conflict. A matching retry returns the already committed result without a second mutation. Evicted or expired keys provide no deduplication guarantee; clients must not automatically retry a create beyond that window.

The [WP03-R2 correction](../agents/reports/WP-03.md#review-corrections-2026-10-13)
adds [migration 3](../../migrations/0003_monitor_identity.sql). Monitor IDs are
opaque signed integers and are never reissued by the service within the persisted
database. The earlier service generated positive SQLite IDs; newly created
monitors use a separate negative namespace and a durable sequence, preserving
even deleted legacy identities whose maximum is no longer recorded. Migration
preserves live IDs/FKs and seeds below existing negative references if any.
Allocation and creation commit together; deletion and retention never reset the
singleton. Retained create keys still reference the original ID; deletion makes
replay conflict, while stale mutations return not found. Key bounds remain
100 rows/24 hours. Sequence exhaustion fails closed without reusing an ID.

## Transactions and worker behavior

WP-03 migration 2 adds current `epoch_id`, `next_due_ms`, cadence, first failure,
compact last result and a separate system-error category to monitor state. The
result key includes the schedule epoch, so reanchors cannot overwrite earlier
physical observations. Legacy foundation results use epoch zero. Preconfigured
foundation monitors start enabled coverage at migration time; no old scheduler
existed from which to infer missing observations.

Each slot immediately and transactionally updates hourly counts; missed slots
advance the durable cursor in chunks of at most 500, returning to normal cadence
after the first missed confirmation. Unknown recent slots retain explicit raw
records for exact seven-day window boundaries. Older history sums hourly buckets
and labels partial boundaries. Retention never deletes raw rows before their
hourly counts commit. A singleton with trigger-maintained row counters enforces
raw/hourly/epoch caps without per-probe history scans. Pressure rejects writes;
it never deletes young incidents. Tombstones hide immediately and clean up in
dependency order with the same 500-row allowance.

Down/recovery transitions insert compact, unique outbox references only when
global delivery and monitor alerts are enabled. Pending row/content overflow
increments `dropped_alerts` in the same incident transaction. API summary exposes
that counter and persisted/local persistence faults. Delivery, digests, retry
and suppression-summary processing remain WP-04.

WP-02 account/session operations share the existing 128-command worker channel
and execution/reply deadlines. Account reads are single-row; each mutation
rechecks checkpoint/disk admission on the worker, then uses a short transaction.
Reset/password change atomically update the hash/generation, delete all sessions
and add bounded audit metadata. Login checks the verified credential generation
again before deleting expired/oldest sessions and inserting the new token hash.
Password change also revalidates its session at commit time. Stale verification
cannot restore access after a credential change. Expired session rows are pruned
by healthy idle maintenance and login. Audit writes retain at most 5000 rows and
30 days. Failed/denied web requests create no durable audit row or per-request log.

Use one database worker/connection initially. Updating a result, state, hourly counters and any incident/outbox transition is one short transaction. A command rejected because the bounded channel is full produces explicit degradation; never acknowledge unsaved configuration. Periodic housekeeping is chunked (up to 500 rows per transaction) and yields to result writes. All web history reads are paginated or aggregated, never full-history exports.

D-19 requires persisting normal/confirmation cadence and the next expected slot
with the result that schedules it. WP-03 must adapt the foundation schema through
a versioned migration if necessary; confirmation checks must not coexist with
replaced normal slots or be counted twice. Missing observations stay unknown;
do not extrapolate hypothetical confirmation chains through downtime. The
[monitoring contract](monitoring.md#scheduling) defines these boundaries.

Proposed settings: foreign keys on, WAL mode, synchronous FULL initially, 2 MiB page-cache target, and a 2-second busy timeout. Benchmark before relaxing durability. Reserve enough deadline budget for reporting database contention. Long queries need cancellation independent of the request timeout.

WAL needs checkpoints, and long read transactions can prevent recycling; keep transactions short. Use the normal automatic checkpoint mechanism initially, plus observed maintenance checkpoints. A 32 MiB WAL allowance is a pressure threshold, not a promised hard cap. Configure recycling limits, stop new history writes before further growth threatens reserved headroom, and surface degraded health if checkpoint progress stalls. Never remove a live WAL file manually. These choices are informed by [SQLite WAL documentation](https://www.sqlite.org/wal.html).

WP-01 maintenance first checkpoints PASSIVE. If the allocated WAL remains at least 32 MiB, it makes one SQLite TRUNCATE checkpoint attempt with lock waiting disabled, within the existing command deadline, then restores the ordinary busy timeout. It remeasures database/WAL/free-space/directory sizes before admitting writes. Pinned readers/writers keep admission blocked; a later maintenance/readiness check retries after they finish, without restarting. PASSIVE completion alone does not reclaim allocated bytes; successful TRUNCATE does, as described in [SQLite checkpoint modes](https://www.sqlite.org/pragma.html#pragma_wal_checkpoint). No retry loop or manual WAL deletion is allowed.

## Retention and size limits

| Data | Proposed retention / bound |
| --- | --- |
| Raw probe results | Seven days; at most 1,010,000 rows globally |
| Hourly aggregates | 90 days; at most 108,050 monitor/hour rows including 50 partial boundary hours (D-23) |
| Incidents | 180 days (confirmed D-10); retain current open incidents. No incident row-count cap (confirmed D-18); existing database/disk-pressure safeguards apply. |
| Schedule epochs | 90 days plus the epoch covering the earliest retained boundary; cap 10,000 rows |
| Audit events | 30 days or 5000 rows, whichever is smaller |
| Sessions | Expire promptly and cap at five |
| Notifications | Bounds in the notification contract |

At baseline, raw retention is `15 * 1440 * 7 = 151,200` result rows; hourly retention is `15 * 24 * 90 = 32,400` rows. Using 128–256 bytes per raw row including indexes gives a planning estimate of roughly 18–37 MiB for raw history. This estimate excludes other tables, fragmentation, and WAL; measure actual pages and file sizes with representative data. Max raw row count corresponds approximately to 50 monitors at 30 seconds for seven days.

Aggregate before deleting raw results. Maintain hourly counters idempotently; reconstruct missed/unknown slots from schedule epochs. Never count a slot twice after crash/retry. Persisted checkpoints of the aggregation horizon make catch-up bounded. For epoch-count pressure, consolidate old epochs only after their hourly coverage is finalized; disclose if detailed coverage can no longer be reconstructed. Do not silently change counts to satisfy a limit.

Enforce a main-database page ceiling corresponding to 512 MiB, validated against its actual page size. Warn at 400 MiB; prune expired data in batches before the ceiling. Incident cleanup must preserve the confirmed 180-day retention and open incidents; pressure does not authorize deleting younger incidents. Use the write-pressure behavior below when space is insufficient. Deletion frees reusable pages but does not necessarily shrink the file; avoid automatic full VACUUM on a small disk.

Check both the application's incremental disk accounting and filesystem free space. Below 256 MiB remaining filesystem space, suspend optional backups/updates, prune eligible history, and warn. Below 128 MiB, stop new history/outbox writes and configuration mutations that cannot be safely persisted; retain a bounded in-memory health indicator and mark observations non-durable/unknown. Resume only after a write/checkpoint health check succeeds. Space needed by unrelated services cannot be guaranteed by this application.

## Migrations and backups

Only the active process migrates at startup before scheduling or serving authenticated data. A failed migration leaves the service unready. Preflight free space and a recoverable backup; never attempt a migration that may exceed the 2 GiB budget.

WP-01 implements transactional migration/version records, application identity and newer/inconsistent-schema rejection before binding. An empty initial database needs no preexisting-data snapshot. Future upgrades require a supported SQLite snapshot at `pre-migration.db`; an existing recovery point blocks replacement until operator review. Preflight reserves worst-case main/WAL growth, the snapshot and the architecture's binary/log/headroom allocations. SQLite defensive mode is enabled, values/rows are limited to 1 MiB, SQL to 64 KiB, columns and bind variables to 128, and attached databases to zero. These ceilings supplement field/row/query bounds; they do not establish complete resource qualification. The worker exposes write-admission health; later mutations must check it within the worker before writing. D-18 removes the proposed incident row-count cap; no separate count-overflow prerequisite remains for WP-03.

A live backup must use SQLite's supported snapshot/backup mechanism, not copy only the `.db` while it is changing. Use the [SQLite online backup API](https://www.sqlite.org/backup.html) incrementally and verify completion. Local staging holds at most one backup; prefer transfer to existing external storage. Restore verification and version rollback are defined in the deployment plan.
