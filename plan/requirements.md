# Product requirements

## Purpose

Provide a private, small-footprint dashboard showing whether the owner's self-hosted applications and server endpoints are reachable from the monitor machine. A successful probe demonstrates the configured endpoint's behavior from this network location, not the health of every component in a server.

## Confirmed owner requirements

| ID | Requirement | Acceptance reference |
| --- | --- | --- |
| R-01 | Service written in Rust | V-01 |
| R-02 | Browser web GUI | V-02 |
| R-03 | Operate within 256 MiB free RAM and 2 GiB free disk | V-07, V-08 |
| R-04 | Login required to access monitoring information and controls | V-03 |
| R-05 | SQLite database | V-04 |
| R-06 | Run on x86_64 Linux; development on Windows | V-01, V-09 |
| R-07 | Design for internet exposure, including flooding, spam, and DDoS precautions | V-06, V-10 |
| R-08 | Day/night GUI with Uptime Kuma as visual inspiration | V-02 |
| R-09 | This agent performs planning only; specifications live under `plan`; owner calls other agents | Planning directory and handoffs |
| R-10 | First release supports HTTP/HTTPS, TCP, and ping | V-05 |
| R-11 | Dashboard plus Telegram alerts on service failure | V-11 |
| R-12 | Monitor 10–15 services; integrate with existing Nginx and WireGuard | V-07, V-10 |

## Proposed first release

These are planning decisions, subject to owner changes in [the decision register](decisions.md).

| ID | Capability | Behavior |
| --- | --- | --- |
| F-01 | HTTP and HTTPS probes | GET request, configurable accepted HTTP status ranges, mandatory TLS verification |
| F-02 | TCP probes | Connect to a configured host and port; close immediately; no arbitrary payload |
| F-10 | Ping probes | ICMP echo using platform adapters, with no root service or shell command execution |
| F-11 | Telegram | Down/recovery alerts, durable bounded retries, test message, visible delivery faults |
| F-03 | Monitor management | Create, edit, pause, resume, delete; confirmation before deleting history |
| F-04 | Status overview | Up, pending failure, down, unknown, paused; explicit stale-data indicators |
| F-05 | History | Recent probe timeline, latency, sample-based availability, coverage, incident list |
| F-06 | Account | One administrator, login/logout, password change, local bootstrap and recovery |
| F-07 | Appearance | Light, dark, and follow-system modes, responsive layout |
| F-08 | Operational visibility | Show scheduler/storage faults separately from target failures |
| F-09 | Resource controls | Bounded concurrency, history, request sizes, queues, sessions, and logs |

Core operation does not depend on browser tabs remaining open. All sensitive GUI pages and data endpoints require an authenticated session. The login page and nonsensitive static assets are necessarily public. No public status page or public signup.

## Scope boundaries

Deferred: DNS-specific probes, push/heartbeat monitors, agents installed on target servers, CPU/RAM metrics, certificate-expiry alerts, content matching, scripted checks, custom request bodies/credentials, public status pages, multiple users/roles, SSO, MFA, discovery, maintenance calendars, plugins, and clustered operation.

Telegram is required for the first release. Email and arbitrary webhooks are deferred. Incident recording and notification delivery are separate operations so Telegram failure never stops checks.

## Proposed capacity and quality targets

The authoritative resource allocations are in [architecture](design/architecture.md). The baseline is 15 monitors at 60-second intervals, five-second total check deadlines, two browser tabs, and a single administrator. Test both successful probes and all-target timeout conditions. Retain the original conservative 256 MiB incremental RAM allowance; the owner's later host figures do not authorize consuming all remaining RAM. The 2 GiB disk budget is a hard requirement.

- New checks normally start within two seconds of their due time at baseline.
- Local API response time is p95 below 300 ms on declared test hardware, excluding probes and password hashing.
- A dashboard refresh occurs every 15 seconds while visible; UI clearly shows last refresh.
- Data loss during a monitor-host outage becomes unknown coverage, never fabricated success.
- Crash recovery, disk pressure, and restart behavior are release requirements.
- Resource targets must be measured on Linux before describing the product as suitable for the small host.

If the monitored server and monitor share a host, power supply, or network connection, one failure can remove both. An independent external check of the monitor is a deployment recommendation; the service cannot report its own complete outage from the failed host.
