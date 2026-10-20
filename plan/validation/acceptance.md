# Acceptance and release evidence

The [WP-00 report](../agents/reports/WP-00.md) records passing Windows/Fedora feasibility checks, measured Hyper header bounds (D-15 resolved), non-root Linux v4/v6 echo under a one-group permission with zero capabilities, and a short Linux capped-memory experiment. WP-00's feasibility gate is complete; none of the complete application release gates below has passed. Future agents can use the [available Linux development machine](../operations/linux-development.md) within their assigned scope. Record commands, environment, observed results and artifacts in work-package reports. A release needs passing applicable gates, not statements that tests should pass.

## Functional and security gates

| ID | Requirements | Evidence required |
| --- | --- | --- |
| V-01 | R-01, R-06 | Windows development build/tests and selected x86_64 Linux release execution; recorded toolchain, dependencies, target and libc baseline |
| V-02 | R-02, R-08 | Login, overview, create/edit/delete, detail, pause/resume, settings; light/dark/system, keyboard navigation, stale/empty/error states, 360 px mobile and desktop screenshots |
| V-03 | R-04 | All private HTML/API routes reject anonymous access; login/session expiry/logout/reset; fixation, CSRF, generic login errors, cookie flags and throttling verified |
| V-04 | R-05 | Transaction rollback, migration failure, crash/restart, backup/restore, foreign keys, bounded retention, incident/outbox atomicity, aggregation idempotency |
| V-05 | R-10, F-01–F-05 | HTTP status/TLS/timeout/DNS, TCP refusal/success, ICMP IPv4/IPv6 and permission faults; threshold/recovery/pause/edit transitions and exact availability/coverage examples |
| V-06 | R-07 | Controlled XSS/injection/CSRF cases; forbidden/allowed private targets, metadata/link-local, rebinding, mixed DNS results, redirects, IPv4-mapped IPv6, spoofed proxy headers; no secret leaks |
| V-07 | R-03, R-12 | Linux memory and sustained scheduler/performance qualification under baseline and fault load |
| V-08 | R-03 | Actual incremental files <= 2 GiB, retention bounds, WAL pressure, full disk, log flood, backup and upgrade temporary-space preflight |
| V-09 | R-06 | Native Windows behavior including ping adapter plus real Linux DNS/TLS/ping, permissions, signals and service lifecycle |
| V-10 | R-07, R-12 | Dedicated Nginx integration/TLS/limits, loopback-only backend, private health routes, existing-site regression checks, WireGuard reachability, non-root ping |
| V-11 | R-11 | Telegram test/down/recovery; digest, ordering, 60-second transient retries, 429 retry-after and durable bot-wide cooldown, permanent failure, restart, expired events, full outbox and possible duplicate handling |

## WP-01 foundation evidence

The [WP-01 report](../agents/reports/WP-01.md) records passing scoped evidence for V-01 and the foundation portions of V-04/V-06/V-08/V-09/V-10. Complete release gates above remain outstanding. This owner-assigned intermediate package requires:

- Windows and real x86_64 Linux fmt, warning-denied clippy, tests, release build and release execution with exact compiler/libc/dependency versions recorded.
- Strict bounded configuration parsing; reject nonloopback binds, insecure production origins, credentials in origins, invalid fields and oversized files. Configuration validation alone must not create state.
- Cross-process ownership rejection, lock release after termination, crash/restart integrity, migration idempotency and preserved configuration. Linux graceful SIGTERM and private permissions; distinguish Windows forced termination from graceful console/service testing.
- Migration failure rolls back DDL/data/version; reject newer/foreign/inconsistent schema before opening the listener. Verify foreign keys, singletons, session/monitor caps, duplicate-slot rejection and SQLite-full rollback. Verify supported snapshot consistency/refusal to overwrite, page-size-based 512 MiB ceiling and conservative migration/backup/disk preflight.
- Nonwaiting database/shared-work admission, queue saturation, expired commands, cancellation/drop cleanup and independent SQL progress cancellation. Test low-space/WAL/checkpoint admission without claiming real retained-history or filesystem-exhaustion qualification. A real SQLite WAL integration case must cross 32 MiB with a pinned reader, remain blocked through bounded maintenance attempts, preserve the reader's snapshot and committed data, then reclaim the WAL and resume admission on the same worker/client after reader release without restart.
- Local-only health, spoofed-header rejection as an identity source, bounded safe errors, absence of private routes, oversized-header rejection and slow-client close. Feature routes added later still require V-03/V-06 authentication and security evidence.

Repeatable runners and retained summary evidence live under [validation/wp01](../../validation/wp01/README.md). These cases clarify the package gate; they do not relax the full release acceptance criteria or the 256 MiB/2 GiB budgets.

## WP-02 security evidence

The [WP-02 report](../agents/reports/WP-02.md) records the owner-assigned backend
security scope. This is scoped V-03 and V-06 evidence, not full release approval:

- Windows and real x86_64 Fedora fmt, warning-denied clippy, tests, release build
  and release execution, with retained environment/exit/test/lifecycle evidence.
- Local commands share persistent ownership, refuse piped/argument credentials,
  have no default/public bootstrap and atomically reset/revoke sessions. Linux
  PTY evidence verifies real prompts, password confirmation/no echo, existing
  account refusal, bootstrap/reset, HTTP login/logout and reset revocation after
  restart. Direct Windows terminal no-echo and graceful Windows console/service
  lifecycle remain explicit unrun platform checks; Windows storage/API lifecycle
  and noninteractive command refusal are tested.
- Fixed Argon2id parameters, random salts/tokens, generic unknown-user/password
  failures, bounded hashing and retained permits after cancellation. Login IP
  map capacity/expiry, global refill/admission, 429/Retry-After, and password
  change's shared global hash budget are tested without uncontrolled traffic.
- All declared private paths/methods and unknown APIs reject anonymous access;
  browser pages redirect. Cookie flags, fixation, signed preauth tamper/expiry,
  canonical Host/Origin, authenticated CSRF, logout/change/reset, persisted
  sessions, exact stored-clock idle/absolute boundaries, oldest eviction and
  stale credential generations are tested. The conservative one-minute activity
  checkpoint behavior is documented in the security contract.
- Trusted proxy identity requires explicit matching socket peer and exactly one
  numeric X-Real-IP; spoofed X-Forwarded values cannot grant access. Credential
  body/header/general-body bounds, malformed input, safe redaction, CSP and
  no-store responses retain foundation protections. Actual account writes fail
  under a pinned WAL and resume on the same worker after reclamation.
- Destination parsing/policy tests cover public/private v4/v6, host/port/ping
  grants, hard-denied metadata/special-use classes, mapped addresses, mixed and
  changed answer sets, answer caps and literal/answer mismatch. Regression cases
  must hard-deny `fd00:ec2::254` for literal HTTP/HTTPS/TCP/ping, hostname DNS
  answers, mixed answer orderings, broad/exact-address/host grants, while preserving
  ordinary granted ULA destinations. WP-03 must reject whole DNS answer sets over
  eight without truncating or filtering; the policy helper's 32-address ceiling
  is a separate defensive bound. WP-03 must add
  live DNS/rebinding, numeric-address connection, redirect and TLS evidence for
  every transport/retry path. SQL is parameterized; full target-data XSS rendering
  belongs to WP-05/WP-07. No Telegram secret or outbound transport exists yet.

Repeatable runners and summaries live under [validation/wp02](../../validation/wp02/README.md).
The full 256 MiB incremental RAM/2 GiB disk envelope, Nginx and WireGuard release
gates remain unchanged and unqualified by these bounded security tests.

## WP-03 monitoring evidence

Review gate closed (2026-10-14): [WP03-R1/R2 re-review](../agents/reports/WP-03-review.md#re-review-and-closure-2026-10-14) confirms the corrections with independent formatting/clippy and 60 Windows/62 Fedora passing tests. WP-03 is accepted for its scoped handoff; WP-04 is ready for separate assignment. Full release qualification remains outstanding.

Correction regressions exercise actual 1,010,000 raw rows and 10,000 epochs,
without modifying admission counters: eligible data must be reclaimed in bounded
transactions during restart/clock-jump recovery, then unknown catch-up and probes
resume. Open and young closed incidents survive; young non-reclaimable pressure
must remain degraded without probes or invented coverage. Monitor identity must
survive deletion/cleanup/restart and version-2 migration: an old create key must
not return a replacement and stale edit/delete must leave it unchanged. Migration
3 uses one durable signed-ID allocator while preserving existing IDs, supported
snapshots and the 100-key/24-hour bound. Five integration regressions and the
version-2 upgrade regression pass on both platforms; final suites have 60 Windows
and 62 Fedora passes, with the Linux namespace-only permission test separately
passed. These are correctness fixtures, not production resource qualification.

[WP-03](../agents/reports/WP-03.md) supplies scoped V-04/V-05/V-06/V-09 evidence:
Windows/Fedora formatting, warning-denied clippy, locked tests, release execution,
account/ownership regression and migration 1-to-2 with a supported recovery
snapshot. Controlled transports cover HTTP/HTTPS wire-header boundaries/shapes,
Host/path/TLS name, trusted/untrusted/wrong-name/expired certificates, redirect
suppression/no body consumption, numeric fallback, whole DNS-set rejection,
private/mapped/metadata denials and native v4/v6 ping. Isolated Fedora permission
denial must be unknown; narrow one-group permission with zero capabilities passes.

State/storage tests cover exact D-19 timing, one incident/down/recovery pair,
duplicate submissions, 20 stale revisions, restart/missed-confirmation normal
coverage, pause exclusion, local faults, clock epochs, exact percentage examples,
idempotency/admission, transactional rollback, outbox suppression, age-based
retention beyond 10,000 recent incidents, open-incident preservation, <=500-row
cleanup and real WAL pressure/recovery on monitoring writes. A controlled-clock
15-monitor runtime test verifies synchronized confirmations/recovery with <=4
probes; it is not a p99 scheduling benchmark or a 24-hour soak.

D-23 permits at most 108,050 hourly rows, including 50 partial boundary hours,
without reducing 90-day retention. Full V-07/V-08 resource, Debian-compatible
V-01/V-09, GUI, Telegram and production integration gates remain outstanding.

## WP-04 notification evidence

The [WP-04 report](../agents/reports/WP-04.md) records implementation, exact
platform commands/results and any unavailable evidence.

Review gate closed (2026-10-18): [WP04-R1 re-review](../agents/reports/WP-04-review.md#re-review-and-closure-2026-10-18) confirms recovery from a committed claim whose reply is lost. Independent formatting/clippy and 91 Windows/93 Fedora tests pass, including all four new regressions. WP-04 is accepted for scoped handoff; full release qualification remains outstanding.

Correction supplied (2026-10-18): [implementation evidence](../agents/reports/WP-04.md#wp04-r1-correction-2026-10-18)
passes four deterministic post-commit reply-loss cases and full 91 Windows/93
Fedora tests, formatting, clippy, release/lifecycle and Linux interactive checks.
The running sender retains admission/degraded health until worker-ordered,
idempotent reconciliation confirms the unsent reservation is released. Cases
include a lost reconciliation reply, expiry/tenth-attempt limits, destination
cancellation and preserved provider cooldown. Existing failed-Finish/WAL and
restart-duplicate regressions still pass. Independent re-review confirms the correction.

Current recorded implementation evidence on Windows and actual Fedora includes:
91 Windows/93 Fedora passing tests, formatting, warning-denied clippy, release
build/execution and lifecycle checks, plus Linux interactive account/session
checks. The ordinary Fedora suite skips one existing namespace-only ping
permission regression; it was not rerun separately for WP-04. Controlled backend
evidence does not close the full release gates below.

Scoped V-11/V-04/V-06 requires controlled fixtures for:

- Atomic incident/event snapshots, five-second coalescing at 15 monitors,
  dated down/recovery ordering, resolved-pair receipts and explicit earlier-alert
  nondelivery. Down-delivery evidence must survive seven-day receipt cleanup.
- Exact completion-plus-60-second transient retries, ten actual/admitted attempt
  bounds and six-hour expiry. Cooldown/pacing waits consume no attempts. A long
  429 wait gates new alerts, retries and tests across restart; shorter waits use
  the 60-second floor, and numeric overflow must never permit an early send.
- Persistent permanent-error stop, corrective revisions/secret changes, durable
  once-per-minute tests, cancellation/audit without queued-destination redirection,
  and the unavoidable already-submitted request window.
- Interrupted remote acceptance/restart with stable logical identity and possible
  duplicate delivery. A temporary acknowledgement-storage failure must keep one
  bounded receipt and resume acknowledgement without another request on the same
  sender after storage recovery. Exercise actual pinned WAL pressure.
- A committed claim whose reply times out or is lost must recover on the same
  sender without a permanently outstanding request. Inject this separately from
  a failed acknowledgement; verify visible degradation until reconciliation,
  bounded state, preserved ordering/cooldowns and resumed subsequent delivery.
- Both 1000-row and 2 MiB pending bounds with incidents preserved, dated suppression
  summary/counters, terminal retention of seven days and at most 2000 rows,
  migration-3 upgrade preservation and a supported recovery snapshot.
- Fixed official-origin HTTPS and verified root/name, numeric fallback before
  POST only, whole DNS-set public-policy validation, no redirects/environment
  proxy, bounded response headers/body and total deadline, plain text/disabled
  previews, safe error categories and no target/query/token leakage. An observed
  429 whose body stalls must preserve its known provider wait.
- Real protected routes: authentication, Origin/CSRF, input/duplicate-field
  validation, stale revisions, signed chat-ID precision, test throttling,
  bounded pagination/no-store/redacted output and separate summary delivery health.

These controlled tests do not prove live channel posting permission, provider
delivery, full GUI visibility, Debian 12 compatibility, 24-hour resources or
production Nginx/WireGuard integration. Those remain release gates. Production
tokens must be provisioned out of the repository; no real channel messages are
required to exercise a controlled backend fixture.

## WP-05 GUI evidence

Owner-assigned WP-05 implementation and scoped V-02/V-03/V-06 evidence are in the
[handoff](../agents/reports/WP-05.md) and [review index](../../validation/wp05/README.md).
Review gate closed (2026-10-20): [WP05-R1/R2/R3 re-review](../agents/reports/WP-05-review.md#re-review-and-closure-2026-10-20) independently confirms live main-pane status/health while controls retain focus, full valid 90-day history after delayed summary use, and newest-incident/recovery/receipt updates across successive polls. Formatting/clippy, 92 Windows/94 Fedora tests, 37 original browser checks and 17 focused regressions pass. WP-05 is accepted for scoped handoff. Preserve unsaved forms, keyboard focus, older-page selection, hidden-tab suspension and bounded requests. The focused suite uses real protected history/incident APIs, controlled SQLite fixtures and browser timer/visibility advancement; server retention and response bounds remain enforced. Full release qualification remains outstanding.

Windows/Fedora formatting, warning-denied clippy, full tests, release build and
controlled lifecycle checks pass. The new regression verifies public nonsensitive
assets, private page shells and a bounded 50-monitor/600-observation summary.

The real Rust HTTP/auth/SQLite browser fixture runs no probes or Telegram sender.
Browser evidence covers responsive 360 px/desktop screens, themes/system changes,
forms, keyboard dialogs, charts/tables, safe hostile names, revisions, actual
pause/resume/delete/account/Telegram APIs, stale/empty/session/error states and
measured theme/control contrast. Explicitly injected server/network faults are
identified in the evidence. These checks are not a complete WCAG certification,
screen-reader review, cross-browser qualification, live Telegram proof, Debian 12
qualification or V-07/V-08 resource soak. Independent release review remains.

## Concrete behavioral cases

- D-14 production qualification must execute the release on Debian 12 x86_64
  with glibc 2.36; Fedora testing does not replace this gate. Record artifact
  library requirements and production-compatible build environment. Do not
  compile or install a toolchain on the monitor host.
- D-21 policy tests must cover all TCP ports via a compact flag on both allowed
  ranges, ports 1/65535 and invalid 0, ungranted private neighbors, independent
  ping permission, conflicting flag/list rejection and unconditional metadata
  denial. This new scope extends the completed WP-02 implementation.
- D-19 cadence: with five-second failures, a first probe due at t=0 completes at
  t=5; confirmations start at t=35 and t=70 and finish at t=40 and t=75. Open
  exactly one incident at t=75. No extra normal check runs at t=60. Without
  scheduler delay, the next normal check is due at t=130. A success on the first
  confirmation cancels the second and resumes normal cadence at t=95.
- Count each confirmation once in availability and expected-slot coverage.
  Replaced normal slots are not missing; missed confirmations, restart,
  pause/edit, clock discontinuity and local faults cannot fabricate failures,
  duplicate slots or replay a confirmation burst. Test the revised admission
  formula and simultaneous pending checks at the 15-monitor baseline.
- Reject HTTP/1 wire response header blocks exceeding 32 KiB through an enforced parser limit; accept the exact boundary and exercise single, cumulative, fragmented, unterminated and excess-count headers over HTTP/HTTPS. WP-00's Hyper tests pass this scoped feasibility case (D-15). The retained test-only reqwest characterization still reproduces the old gap and is not production probe acceptance evidence.
- Retain incidents for confirmed D-10's 180-day window and preserve open incidents. Per D-18 there is no incident row-count cap; crossing 10,000 closed incidents must not itself delete or reject incident records. Verify age-based cleanup and existing disk-pressure safeguards without shortening incident retention.
- Success, failure, failure, failure opens exactly one incident at the third failure; next success closes it. Raw sample-based availability for that sequence is 2/5 = 40%, regardless of incident threshold.
- Four completed results in five expected enabled slots, three successful, show 75% availability and 80% coverage. With no completed result, availability is N/A; with no enabled slot, coverage is N/A.
- Paused time is excluded; a process outage produces unknown slots. Reboot, clock jumps, interval edits and late results cannot duplicate observations or inflate availability.
- A prior open incident survives restart with an observation-gap indication. No immediate fabricated recovery or duplicate logical down event occurs.
- A ping permission error shows monitor-system degradation and does not open target-down incidents. Wrong-source/mismatched echo replies do not pass.
- Twenty saved target edits cannot allow stale in-flight results to overwrite the latest revision.
- Telegram outage leaves the scheduler operational. A resolved incident waiting in the outbox cannot arrive as an undated, apparently current outage. A crash after remote send may duplicate delivery but never loses the durable logical event silently.
- D-22: with a controlled clock, a failed Telegram request finishing at t=0 becomes retryable at t=60, never earlier; a 429 with `retry_after=120` blocks every local bot send, including new alerts/tests, until t=120 even after restart. A shorter provider wait still uses the 60-second floor. Cooldown waits do not consume attempts; the ten-attempt/six-hour limits and permanent-error stop remain enforced. Bark is not required for V-11; its [future acceptance](../design/bark-fallback.md#future-assignment-and-acceptance) belongs to V2-01.

## Resource qualification protocol

Use a representative Linux environment capped to the planned service memory, with no reliance on swap. Record CPU model/count, RAM, filesystem, kernel, distribution, proxy version, release flags, trust roots and starting database size. Load generators/target simulators must run outside the measured service budget and never flood real third-party endpoints.

Baseline: 15 monitors (five HTTP/HTTPS, five TCP, five ping), 60-second normal intervals, five-second deadlines, 30-second confirmation waits below the default three-failure threshold, four probe slots, two visible dashboard tabs polling every 15 seconds, one Telegram channel. Include mixed IPv4/IPv6 where the target environment supports them. Run a 24-hour soak, with representative successful TLS, synchronized pending-failure bursts and timeout phases. Generate retained-history fixtures for seven-day raw, 90-day aggregate and 180-day incident retention.

Measure process RSS, cgroup current/peak including charged cache, allocator growth trend, incremental proxy memory, CPU, scheduler delay, API p95 latency, database/WAL sizes, directory totals, queue depth and dropped/unknown slots. Report at least once per minute and at transitions. Repeated runs are justified only after meaningful changes or unexplained results.

Pass criteria:

- Steady process RSS <= 64 MiB, peak <= 96 MiB including one concurrent password hash; service stays within 128 MiB cgroup maximum with no OOM/restart loop.
- Total measured incremental memory <= 256 MiB including extra proxy overhead; existing sites remain healthy.
- Before production deployment, verify complete-app peak plus proxy overhead
  fits measured normal-load host headroom with an explicit operating margin.
  The owner's 147 MiB available/no-swap snapshot is below the 256 MiB envelope;
  passing that envelope alone does not qualify this host. Reconcile measured
  usage/headroom before enabling the service, preserving other workloads.
- p95 API response below 300 ms excluding login hashing/probes; p99 scheduling delay <= two seconds at baseline.
- Proposed average CPU target <= 5% of one declared core during steady baseline; report actual numbers and hardware rather than advertising an unqualified guarantee.
- Total incremental disk usage <= 2048 MiB during retention, backup staging and a staged upgrade; preflight rejects operations that would exceed it.
- No unbounded trend in memory, rows, queues, logs or active sockets; no silent loss of coverage or alert events.

Fault phases: all targets time out, Telegram unavailable, repeated login failures within permitted local test limits, oversized headers/bodies, slow clients, database busy/checkpoint starvation, bounded storage exhaustion, and controlled service termination. Authentication flood must not multiply Argon2 memory allocations. Resource pressure may reject UI requests, but must remain visible and avoid uncontrolled host impact.

The cap of 50 monitors is an admission safety limit, not a promised tested capacity. Any advertisement/support for more than 15 requires an additional recorded qualification scenario.

## Release evidence bundle

Link automated results, GUI screenshots, resource measurements, dependency/security review, Linux artifact checksum, restore rehearsal, known limitations and installation/rollback docs. Internet deployment is blocked by failed authentication, target-policy, resource-budget, TLS, non-root ping or Telegram acceptance gates. Lower priority UI improvements may be documented as follow-up only if they do not break confirmed requirements.
