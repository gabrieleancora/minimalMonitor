# Data model and persistence

Logical schema only. Implementation agents produce migrations later; no executable schema is included here. Use UTC timestamps, monotonic clocks for in-process elapsed time, parameterized SQL, explicit constraints and versioned migrations.

## Entities

| Entity | Key fields / constraints |
| --- | --- |
| `schema_versions` | Applied migration identifier and timestamp |
| `admin` | Single row; normalized username, Argon2id password hash, credential generation, timestamps |
| `sessions` | Hash of opaque random token, admin ID, CSRF token binding, created/last-used/absolute-expiry times; max five active sessions |
| `monitors` | ID, name, type, validated target fields, interval/deadline, failure threshold, enabled/alerts flags, address family, revision |
| `schedule_epochs` | Monitor/revision, anchor, interval, enabled start/end, coverage discontinuities; enough data to reconstruct expected slots |
| `monitor_state` | One row per monitor: status, failure streak, last due/completed time, last result/error, active incident ID |
| `probe_results` | Monitor/revision, scheduled slot, observed time, outcome, elapsed milliseconds, status code, bounded error category; unique monitor/revision/slot |
| `hourly_stats` | Monitor/hour: expected, successful, failed, unknown slot counts; latency sum/count/max; unique monitor/hour |
| `incidents` | Monitor/revision, first failure time, confirmation time, end time/reason, last evidence, observation-gap flag |
| `notification_outbox` | Event identity, incident, kind, destination revision, compact payload fields, state, attempts, next attempt, creation/expiry, message ID |
| `settings` | Nonsecret global settings, Telegram chat/destination revision, configured public origin |
| `request_keys` | Admin-scoped idempotency key, operation, request digest and committed result reference; at most 100 rows for 24 hours |
| `health_state` | Bounded singleton operational state and counters, including dropped alerts and persistence faults |
| `audit_events` | Bounded security/configuration event metadata; no tokens/passwords or probe payloads |

Foreign keys apply consistently. Index probe results by monitor/time, incidents by monitor/confirmation time, sessions by token hash and expiry, and pending notifications by state/next-attempt time. Delete monitor-owned history in bounded batches after disabling its schedule and canceling queued notifications; hide the tombstoned monitor immediately. No uncontrolled cascade through years of data in an interactive request.

An idempotency key reused with a different request digest is a conflict. A matching retry returns the already committed result without a second mutation. Evicted or expired keys provide no deduplication guarantee; clients must not automatically retry a create beyond that window.

## Transactions and worker behavior

Use one database worker/connection initially. Updating a result, state, hourly counters and any incident/outbox transition is one short transaction. A command rejected because the bounded channel is full produces explicit degradation; never acknowledge unsaved configuration. Periodic housekeeping is chunked (up to 500 rows per transaction) and yields to result writes. All web history reads are paginated or aggregated, never full-history exports.

Proposed settings: foreign keys on, WAL mode, synchronous FULL initially, 2 MiB page-cache target, and a 2-second busy timeout. Benchmark before relaxing durability. Reserve enough deadline budget for reporting database contention. Long queries need cancellation independent of the request timeout.

WAL needs checkpoints, and long read transactions can prevent recycling; keep transactions short. Use the normal automatic checkpoint mechanism initially, plus observed maintenance checkpoints. A 32 MiB WAL allowance is a pressure threshold, not a promised hard cap. Configure recycling limits, stop new history writes before further growth threatens reserved headroom, and surface degraded health if checkpoint progress stalls. Never remove a live WAL file manually. These choices are informed by [SQLite WAL documentation](https://www.sqlite.org/wal.html).

## Retention and size limits

| Data | Proposed retention / bound |
| --- | --- |
| Raw probe results | Seven days; at most 1,010,000 rows globally |
| Hourly aggregates | 90 days; at most 108,000 monitor/hour rows |
| Incidents | 90 days or 10,000 closed incidents, whichever is smaller; retain current open incidents |
| Schedule epochs | 90 days plus the epoch covering the earliest retained boundary; cap 10,000 rows |
| Audit events | 30 days or 5000 rows, whichever is smaller |
| Sessions | Expire promptly and cap at five |
| Notifications | Bounds in the notification contract |

At baseline, raw retention is `15 * 1440 * 7 = 151,200` result rows; hourly retention is `15 * 24 * 90 = 32,400` rows. Using 128–256 bytes per raw row including indexes gives a planning estimate of roughly 18–37 MiB for raw history. This estimate excludes other tables, fragmentation, and WAL; measure actual pages and file sizes with representative data. Max raw row count corresponds approximately to 50 monitors at 30 seconds for seven days.

Aggregate before deleting raw results. Maintain hourly counters idempotently; reconstruct missed/unknown slots from schedule epochs. Never count a slot twice after crash/retry. Persisted checkpoints of the aggregation horizon make catch-up bounded. For epoch-count pressure, consolidate old epochs only after their hourly coverage is finalized; disclose if detailed coverage can no longer be reconstructed. Do not silently change counts to satisfy a limit.

Enforce a main-database page ceiling corresponding to 512 MiB, validated against its actual page size. Warn at 400 MiB; prune expired data and eligible oldest closed history in batches before the ceiling. If emergency pruning shortens configured retention, show the actual retained window. Deletion frees reusable pages but does not necessarily shrink the file; avoid automatic full VACUUM on a small disk.

Check both the application's incremental disk accounting and filesystem free space. Below 256 MiB remaining filesystem space, suspend optional backups/updates, prune eligible history, and warn. Below 128 MiB, stop new history/outbox writes and configuration mutations that cannot be safely persisted; retain a bounded in-memory health indicator and mark observations non-durable/unknown. Resume only after a write/checkpoint health check succeeds. Space needed by unrelated services cannot be guaranteed by this application.

## Migrations and backups

Only the active process migrates at startup before scheduling or serving authenticated data. A failed migration leaves the service unready. Preflight free space and a recoverable backup; never attempt a migration that may exceed the 2 GiB budget.

A live backup must use SQLite's supported snapshot/backup mechanism, not copy only the `.db` while it is changing. Use the [SQLite online backup API](https://www.sqlite.org/backup.html) incrementally and verify completion. Local staging holds at most one backup; prefer transfer to existing external storage. Restore verification and version rollback are defined in the deployment plan.
