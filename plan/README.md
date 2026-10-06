# minimalMonitor planning workspace

Status: initial specification, ready for review; implementation has not started.
Updated: 2026-10-06.

This workspace contains specifications only. The planning agent maintains this directory and does not implement the application, create deployment configurations, or start other agents. The owner explicitly calls implementation agents and selects their assignments.

## Reading order

| Document | Purpose |
| --- | --- |
| [Product requirements](requirements.md) | Confirmed requirements, proposed scope, and success criteria |
| [Decisions and open questions](decisions.md) | Distinguishes owner requirements from planning assumptions |
| [Architecture](design/architecture.md) | Rust components, runtime boundaries, and resource budgets |
| [Monitoring behavior](design/monitoring.md) | Check semantics, scheduling, incidents, and availability |
| [Telegram alerts](design/notifications.md) | Delivery, retries, secret handling, and flood control |
| [Data model](design/data-model.md) | Logical SQLite schema, retention, and failure handling |
| [Web and API contract](design/web-api.md) | Pages, operations, validation, and errors |
| [GUI design](design/gui.md) | Uptime Kuma inspired layout, day/night themes, and accessibility |
| [Security](operations/security.md) | Application, reverse proxy, host, and upstream responsibilities |
| [Deployment and recovery](operations/deployment.md) | Windows development, Linux delivery, backup, and recovery |
| [Acceptance plan](validation/acceptance.md) | Functional, security, compatibility, and resource release gates |
| [Agent instructions](agents/README.md) | Coding conventions, handoff rules, and role assignments |
| [Work packages](agents/work-packages.md) | Ordered jobs the owner can assign to agents |
| [Sources](references.md) | Primary technical references and their use |

## Planning baseline

The proposal is one Rust process with embedded web assets, SQLite, HTTP/HTTPS, TCP and ping checks, Telegram alerts, one administrator, and a Linux service behind the owner's existing Nginx reverse proxy. Browser HTML/CSS and a small amount of JavaScript are allowed; the server is Rust. No Node.js runtime, external database server, or container runtime is required on the monitor host.

The owner expects 10–15 services and already has Nginx and a WireGuard connection to the home network. Qualify against 15 monitors, one check per minute, five-second deadlines, and two open dashboard tabs. Retain a conservative 256 MiB incremental RAM budget; the host has 874 MiB total and approximately 574 MB in use according to htop. The 2 GiB available disk limit is firm. These are design targets, not measured claims.

Probe scope, Telegram, expected scale, and existing infrastructure are confirmed. Defaults such as intervals, retention, and single-admin access remain proposals; the Linux distribution and private target ranges remain open. Consult [decisions](decisions.md).

## Maintaining this plan

- Use requirement IDs when assigning, implementing, and reviewing work.
- Keep authoritative details in the linked document; link to them rather than copying changing defaults.
- Record decisions and dates in `decisions.md`; update affected specifications and acceptance criteria together.
- Future agents place handoff reports in `plan/agents/reports/<work-package>.md`. A report records evidence, changed files, limitations, and remaining decisions.
- A task is not complete because code exists: its acceptance evidence must be present. Do not label unrun checks as passed.

## Change log

| Date | Change |
| --- | --- |
| 2026-10-06 | Initial planning package; no application code, runnable deployment files, or agents created. |
| 2026-10-06 | Owner confirmed ping and Telegram, 10–15 services, existing Nginx/WireGuard, and non-negotiable disk limit. |
