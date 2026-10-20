# WP-03 planning-agent review

Current verdict (2026-10-14): WP03-R1 and WP03-R2 are closed after code review and independent Windows/Fedora checks. WP-03 is accepted for its scoped handoff; WP-04 is ready for a separate owner assignment. No later package or production deployment is started by this review. The original findings and evidence below are retained as history; the latest re-review section supersedes their hold status.

## Review findings

### WP03-R1 — P1: restart recovery can prevent its own retention cleanup

Location: [scheduler](../../../src/scheduler.rs), reanchor branch at lines 107–113 and maintenance at lines 140–142; [storage monitoring](../../../src/storage/monitoring.rs), `missed`, `Reanchor` and `maintain`.

While reanchor is pending, both incomplete catch-up and errors immediately continue the scheduler loop, bypassing `Maintain`. Catch-up inserts raw unknown slots/hourly counts and a new epoch under the existing row ceilings. If these inserts reach a ceiling while expired records still occupy it, the transaction rolls back, reanchor remains pending, and every later tick retries the same failure. The storage worker's idle path only prunes sessions; it cannot release monitoring history capacity. No probes resume even though eligible history could be removed without shortening retention.

A concrete admitted configuration is 50 monitors at a 30-second interval and one-second deadline (load 50/30 < 3). Seven days contain approximately 1,008,000 raw slots, close to the 1,010,000 ceiling. After a one-day process outage, catch-up needs about 144,000 recent unknown slots while roughly a day's old rows are eligible for cleanup. It exhausts the small remaining allowance before reanchor completes, but cleanup is unreachable. Similar behavior is possible at the hourly/epoch ceilings. This concerns existing storage bounds, not a request to restore an incident cap or support more than the qualified 15-monitor performance baseline.

Required correction: allow bounded eligible retention cleanup to make progress before/during restart and clock-jump catch-up, while preserving transactions, unknown coverage, protected retention and write-pressure safeguards. Regression: use a real retained-row fixture near a bound with expired rows and a missed interval, run the scheduler through restart, and assert automatic cleanup, completed reanchor and resumed probes without manual counter changes or restart. Also cover genuine non-reclaimable pressure remaining visibly degraded. R-05/F-08/F-09; V-04/V-08.

### WP03-R2 — P2: monitor ID reuse invalidates stale-request protection

Location: [storage monitoring](../../../src/storage/monitoring.rs), create replay/insert at lines 435–454 and final tombstone deletion at lines 765–766; [foundation schema](../../../migrations/0001_foundation.sql), `monitors.id` and `request_keys`.

Monitor IDs use `INTEGER PRIMARY KEY` with automatic allocation and may be reused after the highest ID is physically deleted. New monitors restart at revision 1. Request-key records retain only a textual monitor ID and are not invalidated by deletion. Create monitor A with key K, delete A and let cleanup finish, then create different monitor B: if B reuses A's ID, retrying K with A's original payload returns B as the successful result. A stale edit/delete carrying A's ID and revision 1 can also match B's revision 1 and change/delete the wrong monitor. Digest validation does not help: it compares the replay against the saved original request, then loads whichever monitor now has that numeric ID.

Required correction: preserve non-reused monitor identity (or an equivalent durable incarnation carried by references and mutation checks), including across deletion/restart/migration; old references must never resolve to a replacement. Keep the bounded 24-hour/100-key policy. Regression: create/delete/cleanup/recreate at the highest ID, replay the old key, and submit stale edit/delete requests; verify B is never returned as A's create result and remains unchanged. R-05/F-03/F-09; V-04 and API revision/idempotency contract.

Both findings are code-path review conclusions. The current tests pass but do not reproduce these combined cases; no new application tests, implementation prototypes or fixes were written during this planning review.

## Validation performed

- Windows: `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --offline -- -D warnings`, and `cargo test --locked --offline` pass; 54 tests, zero failed/ignored. Used the existing stable toolchain and project-local Cargo cache, with rustup auto-install disabled.
- Fedora: accessed the documented isolated WP-03 checkout and reran `cargo test --locked --offline` using existing Rust 1.99.0 and checkout-local caches/temp/target. 56 tests pass, zero failed; one namespace-only permission test remains ignored in the normal suite. The worker's separate narrow-permission evidence was inspected, not rerun here. No source transfer or host configuration change.
- Local and remote retained transfer manifests each validate all 37 selected files with zero mismatches; the inspected scheduler/storage/runtime-test hashes also match across checkouts. Windows/Fedora test counts match the worker's report.
- Existing production-compatibility, resource-soak, GUI and Telegram qualifications remain outstanding as documented. This review did not rerun release packaging, dependency advisory queries or production tests.
- The first SSH attempt could not read the configured key in the sandbox; normal escalation succeeded. A first piped remote test command had a trailing CR argument and did not run tests; the corrected direct command produced the passing results above.
- Before review-document edits, 173 local documentation links resolved and `git diff --check` passed. Checks were repeated after the documentation updates.

## Plan update assessment and handoff

The worker's D-23 updates are relevant: epoch-aware persistence, explicit unknown coverage, all-TCP grants, bounded numeric transports, six blocking helpers and the 108,050-row hourly ceiling describe this package. The extra 50 hourly rows preserve the partial first hour of rolling 90-day retention without changing the firm disk budget. Windows/Fedora evidence is appropriately distinguished from Debian 12/glibc 2.36 and full resource qualification; Bark and Telegram sending remain later scope.

The completion claim was premature in light of WP03-R1/R2. The planning index, work-package status, original report notice and acceptance section now point to this review. Older architecture/API introductory text was reconciled with WP-03's current worker count and readiness behavior. No confirmed requirement was relaxed, application code changed, agent dispatched, commit created or production action taken.

For re-review, provide both targeted regressions, applicable Windows/Linux checks and an updated WP-03 report with actual results. Keep the current incident retention, D-19 timing, D-21 permissions and D-22 Telegram handoff intact.

## Implementation response (2026-10-13; awaiting re-review)

The owner assigned the backend agent to correct WP03-R1/R2. Its
[updated report](WP-03.md#review-corrections-2026-10-13) records bounded maintenance
before pending reanchor, durable non-reused signed monitor identities, actual
row-ceiling recovery and delete/recreate/migration regressions. Final checks pass
on Windows (60 tests) and Fedora (62 tests, plus the separately passed namespace
permission test); confirmed retention, cadence, policy and budgets are unchanged.
This implementation response preserves the original findings/verdict above and
does not constitute planning re-review approval or authorize the WP-04 handoff.

## Re-review and closure (2026-10-14)

No remaining actionable blocker was found in the reviewed corrections and their affected paths.

- **WP03-R1 closed:** `Maintain` now runs in its own admitted transaction before every pending reanchor attempt, so errors/incomplete catch-up cannot bypass retention cleanup. The existing 500-row cleanup bound, age windows and disk/WAL admission remain intact. The regression suite uses actual 1,010,000 raw rows and 10,000 epochs; it verifies reclaimed capacity, committed unknown slots, resumed real probes, preserved incidents, clock-jump recovery and safe degradation when no rows are eligible for removal.
- **WP03-R2 closed:** migration 3 preserves legacy positive monitor IDs and introduces a persistent decreasing negative-ID allocator. Allocation and creation share a transaction; deletion/restart cannot reset the sequence, and exhaustion fails closed. The disjoint range avoids colliding with deleted legacy IDs whose maximum is no longer recorded. Delete/cleanup/restart/recreate and version-2 migration regressions verify old create-key conflicts, rejected stale edit/delete, preserved existing IDs and continued allocation. Monitoring routes/storage use signed IDs, and the API contract tells future clients to treat them as opaque.

Independent verification on 2026-10-14:

| Check | Windows | Isolated Fedora development checkout |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | Pass |
| `cargo clippy --locked --all-targets --offline -- -D warnings` | Pass | Pass |
| `cargo test --locked --offline` | 60 passed; zero failed/ignored | 62 passed; zero failed; one namespace-only permission test ignored |
| Correction coverage | Five integration regressions plus version-2 identity migration test passed | Same six regressions passed |

Used the existing stable toolchains, disabled rustup auto-install, and retained project-local Cargo caches and the documented isolated Linux target/temp paths. The same 39-file transfer manifest matched on both machines, and all selected source/test/runner hashes matched it. No source transfer or host configuration change was needed. The worker's separately passing narrow-namespace test is retained evidence; it was not rerun in this re-review. Release lifecycle, dependency advisory checks and production resource qualification were not repeated.

The plan changes are relevant and consistent with the corrections. They preserve D-18 incident retention, D-19 cadence, D-21 destination policy, D-22 Telegram behavior, and the RAM/disk budgets. The review changes only status/evidence documents. Full Debian 12/glibc 2.36 compatibility, memory/disk/soak qualification, GUI, Telegram sending and deployment remain later gates. WP-04 may now be separately assigned; Bark remains V2-01. No application changes, agents, commit or deployment were made by this review.
