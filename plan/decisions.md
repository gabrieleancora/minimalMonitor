# Decisions and open questions

Updated: 2026-10-06. "Proposed" is a planning baseline, not owner approval or a benchmark result.

## Decision register

| ID | Status | Decision and rationale |
| --- | --- | --- |
| D-01 | Confirmed | Rust service, web GUI, login, SQLite, Linux x86_64, Windows development. |
| D-02 | Confirmed | Work within 256 MiB available RAM and 2 GiB available space. |
| D-03 | Confirmed | Day/night appearance; Uptime Kuma is the GUI reference. |
| D-04 | Confirmed | Planning agent writes specifications only; owner explicitly calls other agents. |
| D-05 | Confirmed | One Rust binary with embedded assets, native Linux service, no required container. |
| D-06 | Confirmed | Axum/Tokio, reqwest with rustls, rusqlite on a dedicated worker, server-rendered HTML and small JavaScript modules. Validate dependency features and actual memory use before committing to this stack. |
| D-07 | Confirmed | HTTP/HTTPS, TCP and ping in the first release. Use unprivileged ICMP where supported; platform feasibility must be demonstrated early. |
| D-08 | Confirmed | One administrator, local first-user bootstrap, cookie sessions, no public registration. |
| D-09 | Confirmed scale; proposed limits | Owner expects 10–15 services. Qualify at 15; propose a hard configuration cap of 50 plus scheduler capacity admission. Higher qualified capacity requires new evidence. |
| D-10 | Confirmed | Seven days raw probes, 90 days hourly aggregates, bounded incidents; sample-based uptime with separate coverage. Incidents log do not get deleted after 90 days but remain for 180 days. |
| D-11 | Confirmed infrastructure; proposed integration | Existing Nginx and WireGuard to the home network. Integrate a dedicated Nginx virtual host and loopback Rust listener; preserve current routing and other sites. |
| D-12 | Proposed | Budget incremental service resources, extra Nginx overhead, data, logs and backup staging. Keep 256 MiB RAM conservative; disk cannot exceed 2 GiB. Host reports: 874 MiB RAM total, approximately 574 MB in use. |
| D-13 | Confirmed | Telegram down/recovery alerts plus dashboard incidents in the first release. |
| D-14 | Proposed | Linux GNU target built/tested in Linux CI or WSL2. Exact distro/glibc baseline is unresolved; do not assume a Windows binary can run on Linux. |

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
