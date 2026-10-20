# Decisions and open questions

Updated: 2026-10-18. "Proposed" is a planning baseline, not owner approval or a benchmark result.

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
| D-12 | Budget retained; production snapshot updated | Keep the 256 MiB incremental RAM ceiling and firm 2 GiB disk limit. On 2026-10-11 the owner reports 873 MiB RAM total, only 147 MiB available, no swap, and 2.9G filesystem space available. Actual headroom and finished-app usage must be qualified before deployment; the ceiling is not a claim that 256 MiB is free. See the production environment record. |
| D-13 | Confirmed | Telegram down/recovery alerts plus dashboard incidents in the first release. |
| D-14 | Production baseline confirmed; artifact verification outstanding | Production is Debian 12 bookworm x86_64, glibc 2.36, kernel 6.1.0-38-cloud-amd64, systemd 252 (PID 1), per owner output on 2026-10-11. See [production environment](operations/production-environment.md). The separate Fedora 44/glibc 2.43 development machine and its [workspace boundary](operations/linux-development.md) remain available. Build and execute a Debian 12-compatible artifact elsewhere; Fedora success alone does not qualify it. |
| D-15 | Resolved by measured WP-00 experiment | Original reqwest 0.12.28 probe accepted 33 KiB headers. Owner requested Hyper: Hyper 1.12.0 HTTP/1 now sets `max_header_size(32768)`, `max_buf_size(32768)` and `max_headers(100)` with verified rustls TLS. Windows/Fedora accept an exact 32 KiB wire header block and reject one byte over, large single/cumulative/fragmented/unterminated blocks and excess header count. The 32 KiB contract was preserved; reqwest is test-only in the prototype and remains proposed for the separate Telegram sender. See [WP-00 evidence](agents/reports/WP-00.md). |
| D-16 | WP-01 implementation baseline, 2026-10-10 | Root application pins Rust 1.98.1 and a committed lockfile. Tests use existing stable installations: Windows 1.98.1, Fedora 1.99.0; no shared toolchain installation. Axum 0.8.9/Tokio 1.53.2/Hyper 1.12.0 remain the foundation stack. rusqlite 0.40.2 bundles SQLite 3.53.2, updating WP-00's older SQLite for later security fixes. A persistent state-directory OS lock is shared by the service and future local account commands; the SQLite thread retains exclusive ownership through close. The intermediate server exposes local health only, with bounded one-request HTTP/1 connections; feature packages must retain or strengthen the same bounds. See [WP-01 evidence](agents/reports/WP-01.md), [runtime foundation](design/architecture.md), [schema](design/data-model.md) and [scoped acceptance](validation/acceptance.md#wp-01-foundation-evidence). No confirmed product/retention/resource requirement changes. |

## D-19: Check cadence (confirmed 2026-10-11)

The owner accepts 60-second normal checks, five-second deadlines and three
consecutive target failures before down. After the first failure, schedule the
remaining confirmation checks 30 seconds apart; success cancels confirmation.
The implementation contract measures each 30-second wait from completion of
the preceding failed check, avoids overlapping/duplicate regular checks, and
returns to the normal cadence after confirmation, success or a local fault.
See [monitoring](design/monitoring.md) for scheduling and coverage details.

Two waits total 60 seconds, not two minutes after the first observed failure.
With five-second check durations, confirmation takes up to about 70 seconds
after that first failed result, or 135 seconds from outage onset including the
initial normal-check wait, before scheduler/delivery delay. The owner's roughly
two-minute alert goal is interpreted through the explicit 30-second cadence;
it is not an exact delivery-time guarantee.

## D-20: Deployment choices (confirmed inputs, 2026-10-11)

The VPS monitors private home endpoints over WireGuard; about 90% of services
are at home. Shared home connectivity failures can make them all unreachable.
Dashboard origin is `https://monitor.gabry.win` with existing wildcard TLS.
Private DNS resolves on the VPS; internal HTTPS uses Let's Encrypt certificates
for `*.local.gabry.win`. Alerts go to a Telegram channel. NAS-initiated SSH pull
backups are being considered; implementation and access details remain open.
See [production environment](operations/production-environment.md) for evidence,
memory headroom, remaining integration checks and backup constraints.

## D-21: Private destination permissions (confirmed 2026-10-11)

The owner permits all TCP ports (1–65535, including HTTP/HTTPS destinations)
and ping within `192.168.1.0/24` and `10.172.73.1/32`. Application enforcement
remains independent of WireGuard routes, with hard-denied destinations and
network/broadcast exclusions preserved. No other private range is authorized.

WP-02 currently accepts at most 64 explicit ports per grant. WP-03 must add a
compact explicit all-TCP option as specified in [security](operations/security.md),
with configuration and policy regression tests, before integrating transports.
Do not expand a range into 65,535 stored ports or interpret an empty list as all.
This is a new owner requirement for the next package, not a missing WP-02 fix.

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

## D-22: Shared Telegram bot and optional Bark fallback (2026-10-11)

Confirmed: the owner shares the Telegram bot with other applications and has a Bark server at home for iPhone notifications. Bark must not replace Telegram. On Telegram failure, try Bark when implemented/configured/available, then retry Telegram after one minute regardless of Bark outcome.

Planning scope: keep Bark in version two, as suggested by the owner; V2-01 remains unassigned. Apply the one-minute transient retry to first-release WP-04 now, replacing the five-second exponential-backoff proposal. Honor a longer Telegram `retry_after` and persist its cooldown across all local bot sends/restarts. Permanent authentication/chat errors retain the existing stop-until-corrected exception, rather than repeatedly sending invalid requests.

The [Telegram contract](design/notifications.md) is authoritative for retry timing and bounds. The [Bark proposal](design/bark-fallback.md) defines a single optional endpoint/device, bounded best-effort fallback, separate provider outcomes and recovery routing. Those detailed v2 defaults are proposals, not owner-specified behavior or implemented features. Bark shares the home's failure domain and cannot replace an external watchdog. Existing RAM/disk limits remain unchanged; WP-03 and later packages are not started by this decision.

## D-23: WP-03 implementation baseline (2026-10-12)

Review status 2026-10-14: [WP03-R1/R2 re-review](agents/reports/WP-03-review.md#re-review-and-closure-2026-10-14) closes both findings after independent Windows/Fedora checks. WP-03 is accepted for its scoped handoff and WP-04 is ready for a separate owner assignment. The choices below remain relevant; these corrections add no feature scope or requirement relaxation.

The owner assigned backend
monitoring. The implementation adds the D-21 all-TCP flag, bounded numeric
transports, persisted normal/confirmation cadence, epoch-safe results, incidents,
sample availability/coverage, bounded cleanup and protected monitoring JSON APIs.
The complete GUI and Telegram sender remain later assignments.

The proposed hourly row ceiling is refined from 108,000 to **108,050**: a rolling
90-day window can intersect 2161 hours, including a partial boundary hour, for
each of at most 50 monitors. Retain that boundary hour until entirely expired;
the fixed extra 50 rows avoid shortening the confirmed retention. This changes
neither the firm disk/RAM envelope nor the incident policy. SQLite/worker caps
remain enforced, and qualification still owes measured retained-history sizes.
The data model and scoped acceptance are updated together.

Runtime blocking helpers rise from two to six to accommodate four admitted
native DNS/ICMP calls, one password hash and storage shutdown. Cancellation keeps
probe admission until native work ends. A two-second wall/monotonic discrepancy
creates an explicit schedule discontinuity. Seven-day raw windows are exact;
older windows use summed hourly counts with partial boundaries labeled. Reports
must distinguish controlled-clock scheduling tests from production latency/soak
qualification and Fedora validation from Debian 12/glibc 2.36 compatibility.

Automatic approval review rejected in-place source updates on the development
host. Validation instead uses an isolated checkout beneath the documented
development project, preserving its existing application source and shared
toolchain. No production software or host-wide network policy changed.

## D-23 review corrections (2026-10-13)

The owner assigned the backend agent to address WP03-R1/R2. Recovery now performs
a separately admitted, <=500-row retention transaction before every pending
restart/clock-jump catch-up attempt. The same write-pressure gates and retention
windows apply; non-reclaimable pressure remains degraded rather than bypassed.

Migration 3 adds a constant-size durable monitor-ID allocator. The prior service
allocated positive SQLite rowids, which can be reused after deleting the maximum;
see [SQLite's rowid contract](https://www.sqlite.org/autoinc.html). Retained rows
cannot reconstruct every deleted legacy maximum. Preserve all existing IDs/FKs
and use a separate negative range with a persisted decreasing sequence for new
monitors. This prevents both future reuse and collisions with deleted legacy IDs
across migration, without an unbounded tombstone table or a changed request shape.
IDs are opaque signed integers; API clients use the value returned by the server.
Create-key limits remain 100/24 hours, and a key referencing a deleted monitor
conflicts. Stale edit/delete requests cannot match a replacement.

Data-model, monitoring and API contracts and the scoped acceptance evidence are
updated with these corrections. No confirmed resource, incident-retention,
confirmation-cadence, destination-policy or Telegram requirement changes.

## Questions awaiting owner input

| Question | Interim assumption | Changes affected |
| --- | --- | --- |
| Will normal-load RAM headroom accommodate the finished app? | 147 MiB available in the current snapshot; retain existing ceilings and measure service plus extra Nginx cost | WP-06/WP-07 deployment qualification, not a user estimate |
| Which NAS backup destination, SSH identity, schedule and remote retention? | NAS-pull direction only; daily and pre-upgrade snapshots remain proposed | WP-06 backup/restore runbook |
| Which exact monitor hostnames and check types? | Private endpoints within D-21's authorized ranges/ports; DNS already resolves | Initial monitor configuration |
| Which backend port and existing Nginx/certificate/DNS details? | Existing proxy and TLS arrangement preserved | WP-06 integration inspection |
| Which Telegram channel ID and bot credentials? | Channel destination confirmed; provision token through a secret file | WP-04/WP-06 setup, no credentials in planning reports |

The owner answered the initial scope questions on 2026-10-06 and supplied production/cadence/network answers on 2026-10-11; those answers are incorporated above. Remaining inputs do not block separately assigned implementation against the confirmed contracts. Settle deployment-specific values before production qualification. Telegram token and destination chat are secrets/configuration to provision later, not information to request in planning chat.

## D-24: WP-04 notification implementation (2026-10-14)

Review status 2026-10-18: [WP04-R1 re-review](agents/reports/WP-04-review.md#re-review-and-closure-2026-10-18) closes the lost-claim-reply finding after independent Windows/Fedora checks. WP-04 is accepted for scoped handoff. The implementation choices below remain relevant; the correction preserves existing delivery requirements and does not add scope.

Owner-assigned WP-04 uses the existing Hyper HTTP/1/rustls stack for fixed-origin
Telegram sending rather than adding the proposed reqwest dependency. It retains
verified public TLS, bounded headers/body/deadlines and whole-set public DNS
policy with at most two numeric pre-POST connection attempts. The runtime adds
one admitted sender DNS helper (seven blocking helpers total). Migration 4 adds
durable provider cooldown/pacing, test throttling, permanent-error latch,
credential-change detection, request acknowledgements, suppression cursor and
terminal timestamps; incident down-delivery evidence survives terminal cleanup.
The API represents chat IDs as canonical decimal strings on read and accepts
validated strings or exact integer input. This implements full signed values
without browser precision loss. See [notification contract](design/notifications.md)
and [WP-04 report](agents/reports/WP-04.md) for evidence and outstanding gates.
No confirmed retry, expiry, retention, resource or provider scope requirement is
weakened. Bark, GUI, deployment and full release qualification remain unassigned.

### WP04-R1 correction (2026-10-18)

The sender retains its one notification permit after an uncertain claim reply
and reconciles the known-unsubmitted reservation on the same serialized worker.
Reconciliation refunds only unsent reservations, preserves terminal cancellation,
expiry, ordering, pacing and provider cooldowns, and atomically acknowledges the
request sequence. Retries are idempotent if reconciliation's reply is also lost.
Known transport outcomes still use Finish and uncertain process restarts retain
their existing duplicate/retry semantics. Health remains degraded until the
running sender receives reconciliation confirmation. No dependency, migration,
helper/queue limit or confirmed notification requirement changes. See the
[worker correction report](agents/reports/WP-04.md#wp04-r1-correction-2026-10-18).

## D-25: WP-05 GUI integration (2026-10-18)

Review status 2026-10-20: [WP05-R1/R2/R3 re-review](agents/reports/WP-05-review.md#re-review-and-closure-2026-10-20) closes all three findings after independent platform/browser checks. WP-05 is accepted for scoped handoff. The implementation choices below remain relevant; these corrections enforce existing behavior without adding infrastructure or relaxing retention/security bounds.

The owner assigned the GUI agent WP-05. Serve embedded, nonsensitive HTML/CSS/JS
with system fonts and no frontend build/runtime dependency. The existing
session/Origin/CSRF/CSP boundary protects all data and page routes. Theme startup
runs synchronously from a local asset before CSS; localStorage failure falls
back to system appearance for subsequent visits.

The additive `recent` summary field supplies at most 12 chronological raw
observations per current monitor (maximum 50), including explicit unknown slots,
through indexed, bounded reads on the existing SQLite worker. It satisfies the
existing single-summary-poll and heartbeat design together. No new endpoint,
migration, worker, dependency or background probe path is introduced. The full
summary remains capped at 256 KiB; the maximum-input regression exercises 50
monitors and 12 observations each.

History uses the agreed aggregated API, with hatched unknown/missing buckets,
separate sample availability/coverage, timestamped keyboard controls and a table.
Incident detail uses bounded global pages and explicitly discloses when this
monitor has no incidents on the current page; receipt absence is unconfirmed
delivery. The persisted API exposes closure reasons, not historical probe error
categories, so the GUI does not invent historical failure detail. These are
presentation details within the existing contract. No confirmed requirement or
resource/retention/security bound is weakened. See [WP-05](agents/reports/WP-05.md).

### WP05-R1/R2/R3 corrections supplied (2026-10-18)

The GUI now reconciles bounded display trees by stable keys while preserving
focus, expanded disclosures and unsaved forms. It does not suspend live main-pane
updates based on focused controls. Newest incident/delivery pages poll on the
existing cadence; older cursor pages explicitly retain a dated snapshot with
same-page refresh. Both incident/receipt reads settle before releasing their
single in-flight guard. No queue, worker, migration or dependency is added.

R2 requires a small additive history query option: exclusive `range_ms`, anchored
and validated against the same current server timestamp. Refreshing a summary
first cannot eliminate network transit at an exact 90-day boundary; arbitrary
client-side padding would omit retained history. Server anchoring preserves the
full requested window, existing explicit-window behavior, 90-day retention,
300-bucket and response-size bounds. See the [correction report](agents/reports/WP-05.md#wp05-r1r2r3-corrections-2026-10-18).
Independent re-review closed the three findings on 2026-10-20; the initial findings are retained as history.

## Tradeoffs to preserve

- Small browser JavaScript is compatible with a Rust backend; a Rust/WASM frontend adds complexity without proving lower host RAM use.
- A single administrator avoids role management, but requires a documented local recovery mechanism.
- Native systemd deployment avoids adding a container daemon to the capacity budget. Containers remain an optional later packaging choice.
- Application bounds protect limited RAM; a reverse proxy and firewall cannot replace secure login, authorization, or outbound-target validation.
- Rate limits mitigate some request abuse; upstream mitigation is required when attack traffic saturates the host's network link.
- More frequent checks and longer history consume resources; validation must reject unschedulable combinations rather than silently degrading.
