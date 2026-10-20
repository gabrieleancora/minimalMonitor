# minimalMonitor planning workspace

Status: WP-00–WP-04 accepted for scoped handoff; WP-05 corrections supplied for WP05-R1/R2/R3; independent re-review pending; WP-06–WP-07 unstarted; full release qualification outstanding.
Updated: 2026-10-18.

This directory contains specifications and handoff reports. The isolated [WP-00 prototype](../feasibility/wp00/README.md) is outside `plan`; its [evidence report](agents/reports/WP-00.md) records completed implementation feasibility and remaining application release gates. The planning agent maintains this directory and does not implement the application, create deployment configurations, or start other agents. The owner explicitly calls implementation agents and selects their assignments.

The owner assigned the Rust backend agent WP-01 and then WP-02 on 2026-10-10.
The [application](../README.md), [WP-01 report](agents/reports/WP-01.md) and
[WP-02 report](agents/reports/WP-02.md) cover the foundation, local accounts,
authentication/CSRF, route protection, input limits and destination policy with
Windows/Fedora evidence. This intermediate build is not a deployable monitoring
release. The owner assigned WP-03 on 2026-10-12; its
[report](agents/reports/WP-03.md) covers monitoring, scheduling, persistence and
policy validation. The [2026-10-14 re-review](agents/reports/WP-03-review.md#re-review-and-closure-2026-10-14) closes both restart-cleanup and monitor-identity findings with independent Windows/Fedora checks. WP-03 is accepted for its scoped handoff. The owner assigned WP-04 on 2026-10-14; its [report](agents/reports/WP-04.md) tracks Telegram implementation and scoped validation. The owner assigned WP-05 on 2026-10-18; its [GUI report](agents/reports/WP-05.md) records implementation and scoped evidence. WP-06–WP-07 remain unstarted.

The [2026-10-18 WP-04 re-review](agents/reports/WP-04-review.md#re-review-and-closure-2026-10-18) closes the uncertain-claim recovery blocker after independent formatting/clippy and 91 Windows/93 Fedora passing tests. The correction adds bounded same-sender reconciliation and four deterministic regressions. WP-04 is accepted for its scoped handoff; WP-05 was subsequently assigned on 2026-10-18; see its [handoff](agents/reports/WP-05.md).

## Reading order

The [WP-05 review](agents/reports/WP-05-review.md) identified focused-view staleness, a failing 90-day range and missing incident refresh. [Corrections and regression evidence](agents/reports/WP-05.md#wp05-r1r2r3-corrections-2026-10-18) are supplied; acceptance remains pending independent re-review.

| Document | Purpose |
| --- | --- |
| [Product requirements](requirements.md) | Confirmed requirements, proposed scope, and success criteria |
| [Decisions and open questions](decisions.md) | Distinguishes owner requirements from planning assumptions |
| [Architecture](design/architecture.md) | Rust components, runtime boundaries, and resource budgets |
| [Monitoring behavior](design/monitoring.md) | Check semantics, scheduling, incidents, and availability |
| [Telegram alerts](design/notifications.md) | Delivery, retries, secret handling, and flood control |
| [Bark fallback — version two](design/bark-fallback.md) | Optional home-hosted fallback; Telegram remains primary; unassigned |
| [Data model](design/data-model.md) | Logical SQLite schema, retention, and failure handling |
| [Web and API contract](design/web-api.md) | Pages, operations, validation, and errors |
| [GUI design](design/gui.md) | Uptime Kuma inspired layout, day/night themes, and accessibility |
| [Security](operations/security.md) | Application, reverse proxy, host, and upstream responsibilities |
| [Deployment and recovery](operations/deployment.md) | Windows development, Linux delivery, backup, and recovery |
| [Production environment](operations/production-environment.md) | Owner-confirmed Debian baseline, resource snapshot, network/TLS, channel and backup inputs |
| [Available Linux development machine](operations/linux-development.md) | Verified SSH access, approved remote folder, Fedora toolchain and repeatable checks |
| [Acceptance plan](validation/acceptance.md) | Functional, security, compatibility, and resource release gates |
| [Agent instructions](agents/README.md) | Coding conventions, handoff rules, and role assignments |
| [Work packages](agents/work-packages.md) | Ordered jobs the owner can assign to agents |
| [Sources](references.md) | Primary technical references and their use |

## Planning baseline

The proposal is one Rust process with embedded web assets, SQLite, HTTP/HTTPS, TCP and ping checks, Telegram alerts, one administrator, and a Linux service behind the owner's existing Nginx reverse proxy. Browser HTML/CSS and a small amount of JavaScript are allowed; the server is Rust. No Node.js runtime, external database server, or container runtime is required on the monitor host.

The owner expects 10–15 services and already has Nginx and WireGuard to the home network. Qualify at 15 monitors, 60-second normal checks, five-second deadlines, 30-second pending-failure confirmation waits and two dashboard tabs. Production is Debian 12 x86_64/glibc 2.36. The latest snapshot has 873 MiB total RAM and only 147 MiB available, with no swap: preserve the 256 MiB incremental ceiling but verify actual headroom before deployment. The 2 GiB incremental disk limit is firm. Resource targets are not measured qualification.

Probe scope, Telegram channel destination, scale, single-admin access, retention, cadence and production platform are confirmed. D-21 authorizes both private ranges for all TCP ports and ping. Remaining work includes concrete service configuration, backup setup and deployment/resource qualification. Consult [decisions](decisions.md) and [production inputs](operations/production-environment.md).

## Maintaining this plan

- Use requirement IDs when assigning, implementing, and reviewing work.
- Keep authoritative details in the linked document; link to them rather than copying changing defaults.
- Record decisions and dates in `decisions.md`; update affected specifications and acceptance criteria together.
- Future agents place handoff reports in `plan/agents/reports/<work-package>.md`. A report records evidence, changed files, limitations, and remaining decisions.
- A task is not complete because code exists: its acceptance evidence must be present. Do not label unrun checks as passed.

## Change log

| Date | Change |
| --- | --- |
| 2026-10-18 | Owner assigned WP-05. Embedded responsive GUI, themes, guarded pages, bounded heartbeat previews, forms, coverage/history, settings and desktop/mobile evidence implemented. Independent review and full release gates remain outstanding. |
| 2026-10-18 | WP04-R1 correction and independent re-review complete. Four injected-reply regressions and 91 Windows/93 Fedora tests pass; formatting/clippy independently pass on both platforms. Worker release/lifecycle evidence retained. WP-04 accepted for scoped handoff; WP-05 ready for separate assignment. |
| 2026-10-14 | Owner assigned WP-04 and authorized source transfer/Linux validation. Telegram backend, protected APIs, durable retries/cooldown and bounded receipts implemented; 87 Windows/89 Fedora tests, release and lifecycle checks pass. One namespace-only ping test is skipped in the ordinary Fedora suite. Scoped WP-04 handoff complete; release qualification remains outstanding. |
| 2026-10-14 | WP-03 re-review closes both blockers after independent formatting/clippy and 60 Windows/62 Fedora tests. WP-04 is ready for separate assignment; full release qualification remains outstanding. |
| 2026-10-13 | WP-03 review reran Windows/Fedora tests and identified restart-cleanup and monitor-ID reuse blockers. Acceptance and the WP-04 handoff remain pending; see the review report. |
| 2026-10-12 | Owner-assigned WP-03 adds all-TCP policy, HTTP/HTTPS/TCP/native ping, protected monitoring APIs, transactional scheduling/coverage/incidents, retention and Windows/Fedora evidence. D-23 documents implementation details and the 50-row partial-hour allowance. Later packages and production remain unstarted. |
| 2026-10-11 | D-22 records shared Telegram bot use, one-minute transient retries and durable provider cooldown; optional Bark fallback is planned as unassigned V2-01. No application changes. |
| 2026-10-06 | Initial planning package; no application code, runnable deployment files, or agents created. |
| 2026-10-06 | Owner confirmed ping and Telegram, 10–15 services, existing Nginx/WireGuard, and non-negotiable disk limit. |
| 2026-10-06 | Owner assigned WP-00. Windows prototype tested; Linux tooling/permission and HTTP header-limit gaps recorded. No subsequent package or production change started. |
| 2026-10-09 | Owner supplied a separate Fedora development machine. WP-00 Linux checks and capped footprint measured; access/workspace documented for future agents. HTTP header-limit gap remains open. |
| 2026-10-10 | Owner-requested Hyper/rustls probe resolves D-15 on Windows/Fedora. Narrow one-group Linux ping proven in an isolated namespace without changing host policy. WP-00 feasibility complete; later packages unstarted. |
| 2026-10-10 | Owner assigned WP-01; backend foundation completed with 18 Windows/19 Fedora tests, release lifecycle checks and dependency/license review. Later packages and production changes remain unstarted. |
| 2026-10-10 | Owner review follow-up adds bounded WAL reclamation and a real pressure/recovery integration test without restart; deployment uses the shared persistent state lock. Final checks pass 19 Windows/20 Fedora tests. WP-02 remains unstarted. |
| 2026-10-10 | Owner assigned WP-02 backend security. Account/session/CSRF, protected routes, request/hash limits, proxy identity and destination policy implemented; 32 Windows/33 Fedora tests, release lifecycle, Linux interactive commands and short capped exercise pass. Later packages remain unstarted. |
| 2026-10-10 | WP-02 owner review hard-denies AWS IPv6 metadata before grants; regression and full checks pass with 33 Windows/34 Fedora tests. Eight remains the monitoring DNS limit; the 32-address policy-helper ceiling is separate. WP-03 incident overflow remains a prerequisite, and WP-03 is unstarted. |
| 2026-10-11 | Owner removed the proposed incident row-count cap (D-18), preserving 180-day retention and existing disk safeguards. The incident-overflow prerequisite is resolved; WP-03 is ready for a separate owner assignment. |
| 2026-10-11 | Owner supplied Debian 12/glibc 2.36 and resource evidence, faster confirmation cadence (D-19), topology/TLS/channel/NAS preferences (D-20), and both private ranges with all TCP ports/ping (D-21). Updated WP-03 policy/scheduling/coverage contracts and Debian/resource qualification gates; no implementation or deployment performed. |
