# Acceptance and release evidence

The [WP-00 report](../agents/reports/WP-00.md) records partial Windows feasibility checks and a read-only Linux permission diagnostic; none of the complete release gates below has passed. Future agents record commands, environment, observed results and artifacts in their work-package reports. A release needs passing applicable gates, not statements that tests should pass.

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
| V-11 | R-11 | Telegram test/down/recovery; digest, ordering, 429 retry-after, 5xx/network retry, permanent failure, restart, expired events, full outbox and possible duplicate handling |

## Concrete behavioral cases

- Reject HTTP response headers exceeding 32 KiB through an enforced parser limit; exercise large single and cumulative headers. WP-00's characterization test reproduces a candidate-client gap and does not pass this acceptance case (D-15).
- Retain incidents for confirmed D-10's 180-day window. Resolve bounded overflow behavior before WP-03; a proposed row ceiling cannot silently substitute 90-day or shorter retention.
- Success, failure, failure, failure opens exactly one incident at the third failure; next success closes it. Raw sample-based availability for that sequence is 2/5 = 40%, regardless of incident threshold.
- Four completed results in five expected enabled slots, three successful, show 75% availability and 80% coverage. With no completed result, availability is N/A; with no enabled slot, coverage is N/A.
- Paused time is excluded; a process outage produces unknown slots. Reboot, clock jumps, interval edits and late results cannot duplicate observations or inflate availability.
- A prior open incident survives restart with an observation-gap indication. No immediate fabricated recovery or duplicate logical down event occurs.
- A ping permission error shows monitor-system degradation and does not open target-down incidents. Wrong-source/mismatched echo replies do not pass.
- Twenty saved target edits cannot allow stale in-flight results to overwrite the latest revision.
- Telegram outage leaves the scheduler operational. A resolved incident waiting in the outbox cannot arrive as an undated, apparently current outage. A crash after remote send may duplicate delivery but never loses the durable logical event silently.

## Resource qualification protocol

Use a representative Linux environment capped to the planned service memory, with no reliance on swap. Record CPU model/count, RAM, filesystem, kernel, distribution, proxy version, release flags, trust roots and starting database size. Load generators/target simulators must run outside the measured service budget and never flood real third-party endpoints.

Baseline: 15 monitors (five HTTP/HTTPS, five TCP, five ping), 60-second intervals, five-second deadlines, four probe slots, two visible dashboard tabs polling every 15 seconds, one Telegram destination. Include mixed IPv4/IPv6 where the target environment supports them. Run a 24-hour soak, with representative successful TLS and fault phases. Generate retained-history fixtures to exercise the full retention window without waiting 90 days.

Measure process RSS, cgroup current/peak including charged cache, allocator growth trend, incremental proxy memory, CPU, scheduler delay, API p95 latency, database/WAL sizes, directory totals, queue depth and dropped/unknown slots. Report at least once per minute and at transitions. Repeated runs are justified only after meaningful changes or unexplained results.

Pass criteria:

- Steady process RSS <= 64 MiB, peak <= 96 MiB including one concurrent password hash; service stays within 128 MiB cgroup maximum with no OOM/restart loop.
- Total measured incremental memory <= 256 MiB including extra proxy overhead; existing sites remain healthy.
- p95 API response below 300 ms excluding login hashing/probes; p99 scheduling delay <= two seconds at baseline.
- Proposed average CPU target <= 5% of one declared core during steady baseline; report actual numbers and hardware rather than advertising an unqualified guarantee.
- Total incremental disk usage <= 2048 MiB during retention, backup staging and a staged upgrade; preflight rejects operations that would exceed it.
- No unbounded trend in memory, rows, queues, logs or active sockets; no silent loss of coverage or alert events.

Fault phases: all targets time out, Telegram unavailable, repeated login failures within permitted local test limits, oversized headers/bodies, slow clients, database busy/checkpoint starvation, bounded storage exhaustion, and controlled service termination. Authentication flood must not multiply Argon2 memory allocations. Resource pressure may reject UI requests, but must remain visible and avoid uncontrolled host impact.

The cap of 50 monitors is an admission safety limit, not a promised tested capacity. Any advertisement/support for more than 15 requires an additional recorded qualification scenario.

## Release evidence bundle

Link automated results, GUI screenshots, resource measurements, dependency/security review, Linux artifact checksum, restore rehearsal, known limitations and installation/rollback docs. Internet deployment is blocked by failed authentication, target-policy, resource-budget, TLS, non-root ping or Telegram acceptance gates. Lower priority UI improvements may be documented as follow-up only if they do not break confirmed requirements.
