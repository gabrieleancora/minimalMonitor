# Architecture and resource envelope

Status: WP-01 foundation and WP-02 security implementation baseline; later
components remain planned. Resource numbers are targets to prove.

WP-01 implements the [backend foundation](../../README.md), using the versions recorded in D-16 and its [report](../agents/reports/WP-01.md). Worker capacities below remain intact. The foundation server uses at most 16 active HTTP/1 connections (including parsing and writes), 16 KiB parser/header budget, a five-second connection deadline and one request per connection; overload closes excess sockets. Two async workers and at most two blocking helpers accompany one dedicated SQLite thread. Shared nonwaiting semaphores reserve four probe slots, one hash and one notification sender; WP-02 uses the hash slot while probe/sender workers remain future packages. Keep bounded connection admission when extending the server in later packages.

## System boundaries

```text
Browser -> existing Nginx (HTTPS) -> loopback Rust HTTP listener
                                      | authentication + HTML/API
                                      | scheduler -> HTTP / TCP / ICMP targets
                                      |               via existing routing/WireGuard
                                      | database worker -> local SQLite
                                      | alert worker -> Telegram HTTPS API
```

One active process owns a database. No remote database, distributed scheduler, target-side agent, message broker, or frontend server. Embed release web assets in the binary. Use a local filesystem for SQLite. A process lock prevents accidentally starting two schedulers against the same state directory.

The WP-01 lock is `owner.lock` in the private state directory, retained without unlinking. This lets later stopped-service bootstrap/reset commands acquire the same identity on Windows/Linux; the database worker owns the lock until it closes its connection. Deployment must retain this shared ownership contract even if a runtime-directory reference is added.

## Candidate stack

| Concern | Proposal | Reason / feasibility requirement |
| --- | --- | --- |
| HTTP server | Axum, Tokio, selected Tower middleware | Async I/O and composable bounds; enable only required features |
| Probe HTTP/HTTPS | Hyper HTTP/1 with rustls | WP-00 validates 32 KiB wire header/parser-buffer limits and 100-header count; verified TLS, one-shot connections, scoped driver and total deadline |
| Telegram | reqwest with rustls | Separate fixed-origin client/policy, verified TLS and bounded response handling to implement/test in WP-04 |
| Database | rusqlite with bundled SQLite | Consistent database version across Windows/Linux; dedicated worker avoids blocking async runtime |
| HTML | Escaping template engine such as Askama | Server-rendered shell, no large SPA framework |
| Browser | Local CSS, small JavaScript modules, simple SVG charts | No CDN, external fonts, telemetry or browser dependencies at runtime |
| Passwords | Argon2id library | See security contract; one hash at a time |
| Ping | Narrow Rust platform abstraction | Linux ICMP datagram sockets; Windows native ICMP API adapter |

These choices are proposals, not selected version numbers. Pin compatible versions and the toolchain during foundation work; audit advisories and licenses at that time. [Axum](https://docs.rs/axum/latest/axum/), [reqwest](https://docs.rs/reqwest/latest/reqwest/), and [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/) provide the candidate capabilities; none demonstrates this application's footprint in advance.

WP-00's isolated Windows and Fedora prototypes support basic stack feasibility, not production qualification. The owner-authorized Hyper follow-up resolves D-15 with measured parser-limit tests on both platforms, verified TLS and short capped Linux release execution. Non-root v4/v6 echo was additionally proven with a one-group permission and zero capabilities in a disposable namespace. The feasibility gate is complete; later application security/retention/soak and production distribution tests remain required. See [the evidence report](../agents/reports/WP-00.md) and [available Linux development environment](../operations/linux-development.md).

## Runtime separation

WP-02 adds one admitted blocking Argon2 task, retaining its permit through
cancellation. Login and password change share the global hash-start limiter;
no waiting hash queue is introduced. Signed preauth is stateless, session rows
remain capped at five and the expiring login IP map at 2048. Destination policy
contains at most 64 grants, with at most 64 explicit ports each; D-21 extends
WP-03 with a mutually exclusive constant-size all-TCP flag, not a larger port
list. Validated answer sets
contain at most 32 supplied addresses as a defensive library ceiling; monitoring
transports retain the stricter eight-answer limit and reject oversized whole
resolutions before calling the policy. Account/session commands reuse the database
worker's channel and worker-side write admission. Existing connection/body/header
deadlines remain intact; credential JSON has a stricter 1024-byte limit.

- Scheduler owns due times and bounded monitor state; four total probe slots, at most one active probe per monitor. D-19's 30-second confirmation waits replace normal slots; admission budgets the faster cadence as defined in the monitoring contract.
- Start with two Tokio runtime workers. Database work runs on one dedicated thread with one connection and a bounded command channel (128 commands). Short queries and batched retention avoid blocking probe result persistence for long periods.
- Password hashing uses a separate blocking task guarded by a one-permit semaphore and no unbounded waiting queue.
- Alert sender has one independent outbound slot, a ten-second deadline, and durable bounded jobs. It does not occupy probe slots.
- UI requests never launch probes directly. Refresh reads persisted/current state; it does not fetch each target.
- Static assets and data responses have explicit size limits. No shared unbounded caches; HTTP idle connection pools and DNS cache entries must be capped.
- Shutdown stops accepting work, cancels checks, drains already-completed results for up to ten seconds, and preserves pending alert jobs. A subsequent start represents the interruption as unknown coverage.

## RAM budget

Budget covers incremental deployment overhead, not the host's pre-existing Nginx/WireGuard memory.

The 2026-10-11 [production snapshot](../operations/production-environment.md)
reports only 147 MiB available with no swap. The allocations below remain
ceilings and qualification targets, not reserved host capacity. Deployment must
fit measured normal-load headroom with an operating margin; a 256 MiB envelope
pass alone cannot establish suitability for this snapshot.

| Component | Proposed allocation |
| --- | ---: |
| Rust process steady-state RSS target | <= 64 MiB |
| Rust process peak RSS, including one Argon2 operation | <= 96 MiB |
| Rust service cgroup high / maximum, including charged file cache | 96 / 128 MiB |
| Incremental proxy and deployment overhead allowance | 32 MiB |
| Remaining incremental headroom at app cgroup maximum | 96 MiB |
| Total incremental RAM envelope | 256 MiB |

RSS and cgroup memory are different measurements; report both. Do not add a duplicate SQLite process budget: SQLite allocations live inside the Rust process, while charged filesystem cache also matters. Existing Nginx must not receive a low process-wide cap that could break the owner's other services. Measure its before/after change. No swap dependency is allowed for passing qualification.

## Disk budget: hard 2 GiB incremental maximum

| Allocation | MiB |
| --- | ---: |
| Current + rollback binaries/assets/configuration | 128 |
| Main SQLite database ceiling | 512 |
| SQLite WAL and working metadata allowance | 32 |
| Application and incremental proxy logs | 64 |
| One local backup or staging snapshot | 512 |
| Unallocated safety/upgrade/filesystem headroom | 800 |
| Total | 2048 |

Do not compile or retain build caches on the monitor host. No unbounded local backup accumulation. A WAL allowance is not enforced simply by setting `journal_size_limit`; active readers can prevent recycling. Control transaction lifetimes, checkpoints, and write admission as described in the data model. The 800 MiB reserve is headroom, not a license for uncontrolled growth.

## Dependency and failure boundaries

Probe outcomes, dashboard health, and notification delivery health are separate. Database failure must not show newly computed results as durably saved. A runtime worker failure triggers visible degraded health and controlled restart if progress cannot resume. A Telegram outage leaves checks operational. A WireGuard outage can cause several real reachability failures from this monitor; group Telegram messages, but do not invent successful target status or assume which network device failed.
