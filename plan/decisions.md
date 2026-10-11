# Decisions and open questions

Updated: 2026-10-11. "Proposed" is a planning baseline, not owner approval or a benchmark result.

## Decision register

| ID | Status | Decision and rationale |
| --- | --- | --- |
| D-01 | Confirmed | Rust service, web GUI, login, SQLite, Linux x86_64, Windows development. |
| D-02 | Confirmed | Work within 256 MiB available RAM and 2 GiB available space. |
| D-03 | Confirmed | Day/night appearance; Uptime Kuma is the GUI reference. |
| D-04 | Confirmed | Planning agent writes specifications only; owner explicitly calls other agents. |
| D-05 | Confirmed | One Rust binary with embedded assets, native Linux service, no required container. |
| D-06 | Confirmed baseline; WP-00 refinement | Axum/Tokio, Hyper HTTP/1 with rustls for probes (owner-authorized D-15 experiment), reqwest/rustls proposed for the separate fixed-origin Telegram sender, rusqlite on a dedicated worker, server-rendered HTML and small JavaScript modules. WP-00 validates bounded probe parsing and short Windows/Linux footprint, not full release qualification. |
| D-07 | Confirmed | HTTP/HTTPS, TCP and ping in the first release. Use unprivileged ICMP where supported; platform feasibility must be demonstrated early. |
| D-08 | Confirmed | One administrator, local first-user bootstrap, cookie sessions, no public registration. |
| D-09 | Confirmed scale; proposed limits | Owner expects 10–15 services. Qualify at 15; propose a hard configuration cap of 50 plus scheduler capacity admission. Higher qualified capacity requires new evidence. |
| D-10 | Confirmed | Seven days raw probes, 90 days hourly aggregates, bounded incidents; sample-based uptime with separate coverage. Incidents log do not get deleted after 90 days but remain for 180 days. |
| D-11 | Confirmed infrastructure; proposed integration | Existing Nginx and WireGuard to the home network. Integrate a dedicated Nginx virtual host and loopback Rust listener; preserve current routing and other sites. |
| D-12 | Proposed | Budget incremental service resources, extra Nginx overhead, data, logs and backup staging. Keep 256 MiB RAM conservative; disk cannot exceed 2 GiB. Host reports: 874 MiB RAM total, approximately 574 MB in use. |
| D-13 | Confirmed | Telegram down/recovery alerts plus dashboard incidents in the first release. |
| D-14 | Development environment available; production baseline open | Owner-provided Fedora 44 x86_64 development machine through `remote-dev`, scoped to `/home/gabriele/rust-develop/minimalMonitor`; Rust 1.99.0/GCC/glibc 2.43 tested on 2026-10-09. See [access/workflow](operations/linux-development.md). Exact production distro/minimum glibc remains unresolved; Fedora success does not establish compatibility with older Linux distributions. |
| D-15 | Resolved by measured WP-00 experiment | Original reqwest 0.12.28 probe accepted 33 KiB headers. Owner requested Hyper: Hyper 1.12.0 HTTP/1 now sets `max_header_size(32768)`, `max_buf_size(32768)` and `max_headers(100)` with verified rustls TLS. Windows/Fedora accept an exact 32 KiB wire header block and reject one byte over, large single/cumulative/fragmented/unterminated blocks and excess header count. The 32 KiB contract was preserved; reqwest is test-only in the prototype and remains proposed for the separate Telegram sender. See [WP-00 evidence](agents/reports/WP-00.md). |
| D-16 | WP-01 implementation baseline, 2026-10-10 | Root application pins Rust 1.98.1 and a committed lockfile. Tests use existing stable installations: Windows 1.98.1, Fedora 1.99.0; no shared toolchain installation. Axum 0.8.9/Tokio 1.53.2/Hyper 1.12.0 remain the foundation stack. rusqlite 0.40.2 bundles SQLite 3.53.2, updating WP-00's older SQLite for later security fixes. A persistent state-directory OS lock is shared by the service and future local account commands; the SQLite thread retains exclusive ownership through close. The intermediate server exposes local health only, with bounded one-request HTTP/1 connections; feature packages must retain or strengthen the same bounds. See [WP-01 evidence](agents/reports/WP-01.md), [runtime foundation](design/architecture.md), [schema](design/data-model.md) and [scoped acceptance](validation/acceptance.md#wp-01-foundation-evidence). No confirmed product/retention/resource requirement changes. |

## D-18: Incident retention without a row-count cap (confirmed 2026-10-11)

The owner removed the proposed 10,000 closed-incident cap and expects roughly
one outage per week at most. This is an expected workload, not an enforced rate
limit. Retain incidents for the confirmed 180-day window and retain open
incidents; impose no separate incident row-count cap or special count-overflow
mechanism. Existing database, disk-pressure, bounded-query and batched-cleanup
controls still apply, including the firm 2 GiB incremental disk budget.

This resolves the incident-overflow prerequisite for WP-03. Earlier report and
migration-comment references to that prerequisite are historical and superseded
by this decision. No application or migration change is needed to remove a cap
that was only proposed. WP-03 remains subject to a separate owner assignment.

## WP-02 security baseline (2026-10-10)

D-17 (WP-02 implementation baseline, 2026-10-10): the owner assigned backend
security only. The [security contract](operations/security.md) now records
interactive bootstrap/reset, fixed bounded Argon2id, stateless HMAC preauth,
hashed durable sessions/CSRF, generation-checked account transactions, cookie
names, explicit HTTP-development handling, canonical Host/Origin and optional
trusted loopback `X-Real-IP`. It also specifies operator-only CIDR/host/port/ping
grants and the validated-address transport handoff. The [API contract](design/web-api.md)
adds the private account-session operation and labels unimplemented feature
responses; [scoped acceptance](validation/acceptance.md#wp-02-security-evidence)
and the [WP-02 report](agents/reports/WP-02.md) retain platform/security evidence
and limitations. No confirmed resource, retention, private-data, protocol or
infrastructure requirement changes. Production proxy/egress inputs and live
transport enforcement remain separately assigned work.

WP-02 owner review reconciliation: `fd00:ec2::254` is hard-denied before ULA
classification and grant evaluation, preserving unconditional metadata denial.
Literal/DNS/mixed/broad/exact-grant regression cases are required. The monitoring
transport DNS limit remains eight answers; the policy helper's 32-address ceiling
is only a defensive library capacity. WP-03 must reject oversized complete
resolutions without truncation/filtering, then validate all accepted answers.
The confirmed 180-day incident retention and firm disk budget remain intact;
the then-open overflow prerequisite is now resolved by D-18 above. This follow-up
does not assign or start WP-03.

## WP-01 review reconciliation (2026-10-10)

D-16's foundation now includes one nonwaiting SQLite TRUNCATE attempt under WAL pressure after PASSIVE maintenance, followed by fresh accounting. Reader/writer contention preserves blocked admission until a later successful check; no restart or manual WAL deletion is needed. See the [storage contract](design/data-model.md), [scoped acceptance](validation/acceptance.md#wp-01-foundation-evidence) and [WP-01 evidence](agents/reports/WP-01.md). The [deployment lock path](operations/deployment.md) is reconciled to persistent `state_dir/owner.lock` for the service and all local database commands; no second runtime lock is permitted. This fixes implementation recovery and stale documentation without changing confirmed requirements or authorizing WP-02.

## WP-00 evidence reconciliation (2026-10-06)

- D-06 remains the candidate stack, with Windows feasibility evidence only; Linux Rust execution and footprint are unverified. WSL Ubuntu 24.04.1/glibc 2.39 is a discovered development environment, not the selected production baseline.
- D-07 is proven for non-elevated Windows IPv4/IPv6 loopback. The WSL non-root diagnostic found denied datagram ping permission for both families (`ping_group_range=1 0`); no sysctl, privilege or infrastructure changes were made.
- Corrected the data model's 90-day incident age to confirmed D-10's 180 days. The proposed closed-incident ceiling and associated overflow question were subsequently removed by D-18.

## Fedora evidence update (2026-10-09)

The 2026-10-09 Fedora rerun superseded the earlier lack of Rust/Linux feasibility evidence: 10 Linux tests and release execution passed, including non-root v4/v6 datagram echo; a temporary 128 MiB scope with swap disabled completed successfully. At that point D-15 remained open. The development host's existing ping permission covers all groups and was not changed.

## Hyper evidence update (2026-10-10)

The owner-authorized Hyper prototype closes D-15: 13 Windows and 14 Fedora tests pass with the required parser limits. A disposable Linux user/network namespace also proves v4/v6 echo with only GID 1000 authorized, UID 1000, zero capabilities and no-new-privileges; the host-wide policy was untouched. WP-00's feasibility gate is complete and WP-01 may be separately assigned. Production still needs its own narrow service group, selected distro/glibc baseline and full application qualification. See [the updated report](agents/reports/WP-00.md).

## Questions awaiting owner input

| Question | Interim assumption | Changes affected |
| --- | --- | --- |
| What check interval is desired? | 60 seconds; scale confirmed as 10–15 | Resource and retention qualification |
| How much additional Nginx overhead is available? | Reserve incremental headroom inside 256 MiB; existing process is already running | Resource qualification |
| Which Linux distribution/version, CPU, and storage? | Systemd-based Linux with local persistent disk | Artifact compatibility, limits, performance |
| Which exact private CIDRs, hosts and ports should be allowed? | Existing WireGuard reaches home; allow only operator-declared private destinations | Outbound destination policy |
| Is the monitor independent of monitored hosts? | Unknown | Reliability expectations and external watchdog |

The owner answered the initial scope questions on 2026-10-06; those answers are incorporated above. Remaining questions do not block independent planning. Settle deployment-specific values before production qualification. Telegram token and destination chat are secrets/configuration to provision later, not information to request in planning chat.

## Tradeoffs to preserve

- Small browser JavaScript is compatible with a Rust backend; a Rust/WASM frontend adds complexity without proving lower host RAM use.
- A single administrator avoids role management, but requires a documented local recovery mechanism.
- Native systemd deployment avoids adding a container daemon to the capacity budget. Containers remain an optional later packaging choice.
- Application bounds protect limited RAM; a reverse proxy and firewall cannot replace secure login, authorization, or outbound-target validation.
- Rate limits mitigate some request abuse; upstream mitigation is required when attack traffic saturates the host's network link.
- More frequent checks and longer history consume resources; validation must reject unschedulable combinations rather than silently degrading.
