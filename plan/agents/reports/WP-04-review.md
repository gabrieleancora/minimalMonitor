# WP-04 planning-agent review

Current verdict (2026-10-18): **WP04-R1 is closed; WP-04 is accepted for its scoped handoff.** Independent code review and Windows/Fedora checks confirm the correction. WP-05 is ready for a separate owner assignment; this review does not start it. The original 2026-10-16 finding and evidence below are retained as history and superseded by the closure section.

## WP04-R1 — P1: a committed claim with a lost reply stalls all future sends

Locations: [sender claim](../../../src/notifications.rs), lines 167–176; [storage notification dispatch and claim guard](../../../src/storage/notifications.rs), lines 107–135, 327–331 and 459–463.

The storage reply has a three-second timeout. A claim transaction changes jobs to `sending` and advances `request_sequence` before returning the batch to the sender. The deadline check immediately before `commit()` does not bound the filesystem commit or guarantee that its result reaches the receiver. For example, a delayed WAL sync can finish after the receiver times out. Dispatch then discards the failed reply send even though the claim committed.

The sender returns from `.await?` without the batch or an acknowledgement to retry. Every subsequent claim returns `None` because `acknowledged_sequence < request_sequence` or a `sending` row exists. Maintenance expires only `pending` rows, so six-hour expiry cannot release this outstanding request. Canceling rows through settings also leaves the sequence outstanding. Only process startup reconciles it. This blocks all later alerts and test messages until restart; a later successful empty claim also clears `persistence_degraded`, concealing the fault.

Required correction: reconcile an uncertain claim outcome on the same running sender, using bounded state and preserving single-sender admission, attempt/expiry limits, event ordering, destination cancellation and provider cooldowns. Do not simply remove the outstanding-request guard or blindly replay a possibly submitted request. Keep delivery health degraded until the uncertainty is resolved.

Required regression: deterministically commit a claim and withhold/drop its response past the caller deadline, then restore storage responsiveness. Assert that the same sender recovers without restart, no orphan `sending` row/request sequence permanently blocks subsequent alerts, health reflects the unresolved interval, and transport is not invoked twice for a batch known not to have reached the sender. Retain the existing failed-`Finish`/WAL regression and restart duplicate semantics. Coverage: R-11/F-11, R-05; V-11/V-04 and the delivery-health contract.

This is a code-path finding, not a newly executed fault-injection reproduction. The current passing tests cover a failed acknowledgement after the sender already knows its batch; they do not cover a committed claim whose reply is lost. No application code, prototype or new implementation test was written during this planning review.

## Independent verification

| Check | Windows | Isolated Fedora development checkout |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | Pass |
| `cargo clippy --locked --all-targets --offline -- -D warnings` | Pass | Pass |
| `cargo test --locked --offline` | 87 passed; zero failed/ignored | 89 passed; zero failed; one namespace-only ping test ignored |

Used the installed stable toolchains, disabled rustup auto-install, and retained project-local Cargo caches. Linux checks used the existing authorized `remote-dev:/home/gabriele/rust-develop/minimalMonitor/validation/wp04/workspace`, its target/temp directories and the WP-03 dependency cache. All 44 selected source/test/config/runner files matched the retained normalized-byte transfer manifest locally and remotely; manifest SHA-256 was `e9fb54adbc5f34ccc433b435468c0c35b094f2be333b522dac72f7be08a38753`. No source transfer or host configuration change was needed.

The worker's release/lifecycle and interactive-account evidence was inspected, not rerun. The namespace-only permission test was not rerun separately. No live Telegram request, real credential provisioning, production access, Debian compatibility check or resource soak was performed. Existing release gates remain open. Documentation link and whitespace checks were repeated after the review updates.

## Plan assessment and handoff

D-24 and the accompanying contracts are relevant: reuse of Hyper/rustls avoids another production HTTP dependency; seven blocking helpers account for sender DNS; durable cooldown/receipt metadata and decimal-string chat IDs support the existing requirements. Retry timing, shared-bot cooldown, resource bounds and Bark's deferred scope are preserved. Windows/Fedora correctness evidence is appropriately separated from Debian and full resource qualification.

The completion wording was premature because of WP04-R1. The planning index, work-package gate, worker report notice, decision register and acceptance criteria now point here. The notification failure contract explicitly includes uncertain claim replies. The required correction is recovery within the existing notification scope, not a new feature or higher capacity target.

For re-review, supply the focused regression, applicable Windows/Linux checks and updated WP-04 evidence. No source changes, other-agent dispatch, commit, push or deployment were made by this review.

## Implementation follow-up (2026-10-18)

The owner assigned the Rust backend agent to implement this finding. The
[correction report](WP-04.md#wp04-r1-correction-2026-10-18) supplies bounded
same-sender reconciliation, four deterministic committed-reply-loss regressions,
91 Windows/93 Fedora passing tests, release/lifecycle and Linux interactive checks.
The prior dated planning verdict and independent evidence above are preserved.
This is implementation evidence supplied for re-review, not an independent
closure of WP04-R1 or authorization to start another package.

## Re-review and closure (2026-10-18)

No remaining actionable blocker was found in the correction and affected paths. **WP04-R1 is closed.**

The sender retains its single notification permit before awaiting a claim. If the reply is lost, it reconciles on the same serialized SQLite worker before admitting another claim or entering transport. The reconciliation transaction releases only known-unsent reservations, preserves terminal cancellation and expiry, and acknowledges the request sequence atomically. A second reconciliation after a lost reconciliation reply is a no-op, so it cannot refund an attempt twice. Provider cooldown and pacing remain intact; the sender keeps degradation visible until it receives confirmation. Known delivery outcomes continue through the existing acknowledgement path, and restart uncertainty retains the documented at-least-once semantics.

The four new tests use the actual SQLite worker and commit before deliberately withholding a reply past the caller's deadline. They verify same-sender recovery and subsequent delivery, persistent degradation/admission while unresolved, lost-reconciliation-reply idempotency, the tenth-attempt and six-hour expiry boundaries, and destination cancellation with preserved provider cooldown. The injection is test-only. Existing failed-acknowledgement/WAL, ordering and restart regressions remain passing.

Independent checks on the corrected sources:

| Check | Windows | Isolated Fedora development checkout |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | Pass |
| `cargo clippy --locked --all-targets --offline -- -D warnings` | Pass | Pass |
| `cargo test --locked --offline` | 91 passed; zero failed/ignored | 93 passed; zero failed; one namespace-only ping test ignored |
| New reply-loss regressions | All four passed | All four passed |

Used the installed stable toolchains with rustup auto-install disabled and the existing project-local caches/target/temp paths. All 44 selected files matched the current local transfer manifest on both machines; normalized manifest SHA-256: `ba32f437abc70d478b03e9e1701c073b5cbc9ac9c8d0dd0cef9b858491b672a0`. The initial remote check looked for a manifest at a path where it was absent and stopped before tests. The corrected check supplied the local manifest in memory and verified every remote file before running checks. No source transfer or host configuration change was needed.

Reviewed the worker's updated release/lifecycle and Linux interactive summaries; those checks were not independently rerun. The namespace-only permission test was not rerun separately. No live Telegram, production access, Debian compatibility or resource-soak check was performed. Those existing release gates remain outstanding.

The plan changes accurately describe this recovery correction and preserve confirmed retry/cooldown rules, resource bounds and deferred Bark scope. Updated current status in the index, work packages, worker report, decision register and acceptance plan; documentation links and whitespace checks pass. No application code, tests, other-agent dispatch, commit or deployment were created by this review.
