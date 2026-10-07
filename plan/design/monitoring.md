# Monitoring behavior

## Configuration

| Field | Initial rule |
| --- | --- |
| Name | Required; 1–100 characters, displayed as escaped text |
| Type | HTTP, TCP, or ping |
| Target | Structured validated fields; HTTP URL or host plus port for TCP; host for ping |
| Interval | Default 60 seconds; range 30–3600 seconds |
| Total deadline | Default 5 seconds; range 1–30 seconds and strictly less than interval |
| Failures before down | Default 3 consecutive completed target failures; range 1–5 |
| Recovery | First subsequent successful check |
| Enabled | New monitors enabled unless explicitly saved paused |
| Alerts | Enabled by default when global Telegram delivery is enabled |
| HTTP status | Default 200–299; configurable finite list/ranges within 100–599 |
| Address family | Auto, IPv4, or IPv6; record actual destination family |

Hard cap: 50 configured monitors including paused ones. Admission for enabled monitors requires the sum of `deadline_seconds / interval_seconds` to be <= 3, reserving capacity within four probe slots. Validate create, edit and resume. This is a conservative scheduling rule, not a substitute for the 15-monitor performance qualification.

## Probe definitions

HTTP: send GET and evaluate response headers/status; do not download or store response bodies. No redirect following in version one: a 3xx passes only if explicitly accepted. Disable automatic decompression, cookie persistence, environment proxy inheritance and automatic retries. Cap received headers at 32 KiB; enforce the total deadline across DNS, connection and TLS. Never permit insecure TLS mode. Operator-managed additional trust roots may support private HTTPS certificates.

The header bound must apply while parsing, before accepting an oversized response; checking the final header map alone does not bound parser allocation. WP-00 found the candidate reqwest HTTP/1 configuration accepting 33 KiB. Resolve D-15 with a demonstrably bounded transport without weakening this limit.

TCP: DNS plus connection establishment inside one deadline. Success means a TCP connection was accepted, not that the application protocol worked. Send no payload and close promptly.

Ping: send one fixed, small ICMP echo payload per selected destination attempt; validate reply type, source, correlation and echoed payload. Success proves echo responsiveness. Permission errors or unavailable platform support are monitor-system errors, not evidence a server is down. No shell, external `ping` output parsing, or root service. Linux feasibility must prove datagram ping sockets with narrowly assigned group permission. Windows uses a native ICMP adapter, validated alongside the production Linux adapter. [Linux kernel guidance](https://docs.kernel.org/networking/ip-sysctl.html) describes `ping_group_range`; Microsoft's APIs distinguish [IPv4](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho2) and [IPv6](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2) echo requests.

For all protocols, DNS answers and connection attempts pass the outbound policy. Cap accepted DNS addresses at eight and reject oversized answer sets. Auto mode may try up to two validated addresses sequentially, splitting the remaining deadline, with success if either meets the check contract. No second unchecked DNS lookup. Latency is end-to-end monotonic elapsed time, including DNS and fallback; it is not represented as pure ICMP round-trip time. Store individual error categories without remote response bodies.

## Scheduling

Spread initial due times across one interval using bounded jitter; then use monotonic due times. Each interval has at most one scheduled result. Never overlap checks, build an unbounded queue, retry immediately on failure, or replay missed checks after downtime. Expired slots are unknown coverage. Persist enough schedule/configuration epochs to reconstruct expected slots across restarts. Wall-clock jumps must create an explicit coverage discontinuity and re-anchor the schedule, without duplicate slots.

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

With defaults, sustained failure detection takes up to approximately 185 seconds plus scheduler delay (one interval to the first probe, two more intervals, one deadline). A 15-second UI refresh can add display delay; Telegram timing is separately specified.

## Availability and coverage

Version one reports **sample-based availability**, not an exact time-based SLA: successful scheduled probes / (successful + failed scheduled probes). Pending failures count as failed samples even before an incident opens. DNS resolution failures and remote timeouts count as failures; local permission/policy/DB/scheduling faults are unknown.

Coverage is completed valid target-result slots / expected enabled slots. Paused periods are excluded; missed checks, process outages, and local faults reduce coverage. Denominator zero produces `N/A`, never 100%. Display sample count and coverage next to percentages so partial history is visible. Aggregate counts by summing numerators and denominators; never average percentages. Compare only the same declared time window; partial first/last hourly buckets are labeled as such for older history.
