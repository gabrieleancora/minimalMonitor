# Monitoring behavior

## Configuration

| Field | Initial rule |
| --- | --- |
| Name | Required; 1–100 characters, displayed as escaped text |
| Type | HTTP, TCP, or ping |
| Target | Structured validated fields; HTTP URL or host plus port for TCP; host for ping |
| Interval | Confirmed normal default 60 seconds; configurable range 30–3600 seconds |
| Total deadline | Confirmed default 5 seconds; range 1–30 seconds and strictly less than normal interval |
| Failures before down | Confirmed default 3 consecutive completed target failures; configurable range 1–5 |
| Pending-failure confirmation | Fixed 30-second wait after each failed check completes while below threshold; replaces the next normal check |
| Recovery | First subsequent successful check |
| Enabled | New monitors enabled unless explicitly saved paused |
| Alerts | Enabled by default when global Telegram delivery is enabled |
| HTTP status | Default 200–299; configurable finite list/ranges within 100–599 |
| Address family | Auto, IPv4, or IPv6; record actual destination family |

Hard cap: 50 configured monitors including paused ones. Admission for enabled monitors requires the sum of `deadline_seconds / effective_interval_seconds` to be <= 3, reserving capacity within four probe slots. Use `effective_interval_seconds = min(normal_interval_seconds, 30)` when the failure threshold exceeds one; otherwise use the normal interval. This conservatively includes accelerated confirmation load. Validate create, edit and resume. The 15-monitor, five-second-deadline baseline uses 2.5 of this allowance. This is not a substitute for measured scheduling qualification.

## Probe definitions

HTTP: send GET and evaluate response headers/status; do not download or store response bodies. No redirect following in version one: a 3xx passes only if explicitly accepted. Disable automatic decompression, cookie persistence, environment proxy inheritance and automatic retries. Cap received headers at 32 KiB; enforce the total deadline across DNS, connection and TLS. Never permit insecure TLS mode. Operator-managed additional trust roots may support private HTTPS certificates.

The header bound applies to the complete HTTP/1 wire header block, including the status line and CRLF terminator, and must be enforced during parsing; checking the final header map alone does not bound parser allocation. Set a 32 KiB header-size and connection-buffer strategy limit plus a 100-header count limit. WP-00's Hyper/rustls transport accepts exactly 32 KiB and rejects oversized single, cumulative, fragmented and unterminated blocks on Windows/Fedora (D-15 resolved). Probe HTTP/1 support does not require HTTP/2, connection pooling or body downloads.

TCP: DNS plus connection establishment inside one deadline. Success means a TCP connection was accepted, not that the application protocol worked. Send no payload and close promptly.

Ping: send one fixed, small ICMP echo payload per selected destination attempt; validate reply type, source, correlation and echoed payload. Success proves echo responsiveness. Permission errors or unavailable platform support are monitor-system errors, not evidence a server is down. No shell, external `ping` output parsing, or root service. Linux feasibility must prove datagram ping sockets with narrowly assigned group permission. Windows uses a native ICMP adapter, validated alongside the production Linux adapter. [Linux kernel guidance](https://docs.kernel.org/networking/ip-sysctl.html) describes `ping_group_range`; Microsoft's APIs distinguish [IPv4](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho2) and [IPv6](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2) echo requests.

For all protocols, DNS answers and connection attempts pass the outbound policy. Cap accepted DNS addresses at eight and reject oversized answer sets. Auto mode may try up to two validated addresses sequentially, splitting the remaining deadline, with success if either meets the check contract. No second unchecked DNS lookup. Latency is end-to-end monotonic elapsed time, including DNS and fallback; it is not represented as pure ICMP round-trip time. Store individual error categories without remote response bodies.

The eight-answer transport limit remains authoritative for HTTP/HTTPS, TCP and
ping. WP-02's shared policy helper has a separate defensive capacity of 32
supplied addresses; that ceiling does not increase the monitor's DNS allowance.
WP-03 must reject the whole resolution if it contains more than eight answers,
without truncating or filtering before policy evaluation. Validate every accepted
answer before address-family/fallback selection; a denied ninth answer cannot
be hidden by keeping the first eight. No resolver/transport is implemented by
this WP-02 clarification. See the [security contract](../operations/security.md#outbound-destination-policy).

## Scheduling

Spread initial due times across one normal interval using bounded jitter; then use monotonic due times. Each due slot has at most one scheduled result. Never overlap checks, build an unbounded queue, retry immediately on failure, or replay missed checks after downtime. Expired slots are unknown coverage. Persist enough schedule/configuration epochs to reconstruct expected slots across restarts. Wall-clock jumps must create an explicit coverage discontinuity and re-anchor the schedule, without duplicate slots.

D-19 adds scheduler-owned confirmation checks, not hidden transport retries.
While a completed target failure is below the configured threshold, replace the
next normal due time with completion time plus 30 seconds. With the default
threshold, at most two confirmation checks follow the first failure. Cancel the
old normal due slot instead of running both schedules. A success resets the
streak; confirmation of down or a local/system fault also exits confirmation
mode. Resume normal cadence anchored to the last scheduled probe's due time.
Down monitors continue normal checks, with recovery on the first success.

Persist the mode and next due slot together with each result/transition, so
confirmation observations contribute exactly once to samples and coverage.
Replaced future normal slots are not missed observations. A missed confirmation
slot is unknown and breaks the streak; return to normal cadence. Restart never
replays pending confirmations or infers hypothetical fast checks during downtime:
record the known missed slot and an observation gap, reset the streak and resume
normal cadence. After a missed confirmation slot, reconstruct subsequent missed
slots at the normal interval from that slot's due time until a recorded schedule
change/restart re-anchor. This keeps downtime in the coverage denominator without
inventing further confirmations. WP-03 must preserve these boundaries in its
epoch/coverage model.

During restart and clock-jump reanchoring, run separately admitted retention
transactions before each catch-up attempt. Each deletes at most 500 eligible
history rows, including tombstone cleanup; normal housekeeping remains periodic.
Incomplete catch-up and insert-capacity errors cannot bypass housekeeping.
Recovery remains degraded until it commits and scheduler progress resumes.
Non-reclaimable row capacity and genuine disk/WAL pressure remain visible faults;
do not relax admission, delete protected incidents, or fabricate coverage to
force a reanchor.

After an edit affecting probe behavior, increment its configuration revision, cancel old work, discard late results from the old revision, reset failure streak, and show unknown until the new configuration is checked. Close an existing incident as `configuration_changed`, not recovered. Pause cancels active work and ends the incident as `paused`; resume starts a new schedule epoch and unknown state.

## State and incidents

| Current state / event | Result |
| --- | --- |
| New/resumed/reconfigured | Unknown until first result |
| Successful result | Up; reset consecutive failures |
| Failure below threshold | Pending failure, showing count and reason |
| Failure reaches threshold | Down; open exactly one incident and enqueue one logical down event |
| More failures while down | Update last evidence; keep same incident; no per-check alert |
| Success while down | Up; close incident and enqueue recovery event |
| Operator pause | Paused; no outage/recovery alert; record explicit incident end reason |
| Permission, scheduler, policy, or persistence fault | Unknown/degraded system health; reset failure streak, do not claim target down |

After `2 * interval + deadline` without a valid completed result, display unknown/stale with the last known result and timestamp. A previously open incident remains unresolved across unknown periods; its duration includes an explicitly identified observation gap. Restart resets the failure streak, preserves open incidents, and waits for fresh evidence before recovery. Recovery after a gap says when service was observed recovered, not when it actually recovered.

With defaults and no scheduler delay, sustained failure confirmation takes up to approximately 135 seconds from outage onset: up to 60 seconds until the first probe, three checks of up to five seconds, and two 30-second waits. From the first completed failure it takes 60–70 seconds. This implements the owner's faster checks, not a literal two-minute delay after first observation. A 15-second UI refresh can add display delay; Telegram submission/delivery timing is separately specified and must not be advertised as an exact two-minute guarantee.

## Availability and coverage

Version one reports **sample-based availability**, not an exact time-based SLA: successful scheduled probes / (successful + failed scheduled probes). Pending failures and confirmation checks count as individual samples even before an incident opens. The faster pending cadence samples unstable periods more often; the percentage is not time-weighted uptime. DNS resolution failures and remote timeouts count as failures; local permission/policy/DB/scheduling faults are unknown.

Coverage is completed valid target-result slots / expected enabled slots. Paused periods are excluded; missed checks, process outages, and local faults reduce coverage. Denominator zero produces `N/A`, never 100%. Display sample count and coverage next to percentages so partial history is visible. Aggregate counts by summing numerators and denominators; never average percentages. Compare only the same declared time window; partial first/last hourly buckets are labeled as such for older history.
