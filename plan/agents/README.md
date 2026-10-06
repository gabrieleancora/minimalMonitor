# Instructions for future agents

The owner explicitly calls agents. This document defines roles and assignments; it does not authorize the planning agent to implement or automatically dispatch them. All work packages begin unassigned. Development/operations agents act within the particular assignment the owner gives them.

## Shared workflow

1. Read `plan/README.md`, requirements, decisions, and your work package before changing files.
2. Identify applicable requirement IDs, dependencies and unresolved decisions. Resolve routine implementation details within the stated baseline; raise contradictions rather than silently expanding scope.
3. Check the actual repository and any owner-provided instructions. Preserve others' work. Do not assume Git, CI, tooling or deployment credentials already exist.
4. Keep code, tests and executable deployment assets outside `plan`; this folder is specifications and reports only.
5. Update the affected contract when implementation reveals a necessary change, recording rationale in decisions. Confirmed owner requirements cannot be silently weakened.
6. Supply acceptance evidence appropriate to the assignment. Never claim production performance from a development machine alone.
7. Write `plan/agents/reports/<package-id>.md` with completed scope, files, test evidence, resource/security implications, deviations, unresolved issues and the next handoff.

## Roles

| Role | Ownership | Important boundary |
| --- | --- | --- |
| Planning agent | Requirements, design contracts, decisions, backlog, handoff definitions | No application development or deployment; no automatic agent dispatch |
| Rust backend agent | Runtime, probes, database, authentication, API, Telegram | Follow bounded-resource and outbound-policy contracts |
| GUI agent | Templates, local assets, responsive design, accessible themes/charts | Coordinate API before changes; no heavy runtime/framework by default |
| DevOps agent | Linux packaging/service, scoped Nginx integration, ping permissions, backup/restore, Windows/Linux CI | Preserve existing Nginx/WireGuard; no production changes beyond owner authorization |
| QA/security agent | Adversarial functional tests, platform verification, resource evidence and release review | Use controlled targets; do not perform destructive real-network testing |

One agent can perform multiple roles when explicitly assigned. Parallel development is not assumed; backend/GUI boundaries can be coordinated after API contracts and foundation evidence exist.

## Coding instructions

- Keep the initial project a small modular Rust application; avoid a multi-service architecture. Suggested boundaries: config, auth, web, scheduler, probes, destination policy, storage, notifications, health.
- Use stable Rust, a committed application lockfile, minimal dependency features, explicit errors, and parameterized database calls. Record dependency/version/security choices in the foundation report.
- Keep blocking SQLite and password work off async runtime workers. Bound every channel, task family, pool, cache, request body, result set and persistent queue.
- Prefer safe Rust. Isolate unavoidable Windows ICMP FFI/low-level socket operations with documented safety invariants and tests; no shell execution for target checks.
- Inject clocks, DNS resolution, transports and storage boundaries for deterministic timeout/state tests. Avoid retry loops or polling that make tests flaky.
- Use platform paths/configuration and conditional adapters deliberately; no hard-coded developer paths. Production secrets never enter source, fixtures, snapshots or logs.
- Authentication and destination policy are shared middleware/services, not copied per handler. GUI validation supplements server validation.
- Do not add public registration, anonymous data, arbitrary scripts, target credentials, external chart/font dependencies or unrelated framework features.
- Run formatting, lints, relevant unit/integration tests and required platform checks. Add meaningful tests for state machines, concurrency bounds, persistence and security behavior, not tests that merely repeat constants.
- Mark benchmark and security claims with their measured environment and limits. If a proposed stack cannot meet resource targets, return evidence and alternatives to planning before broad implementation.

## Report template

Each report contains: package ID/status; owner assignment; requirement coverage; decisions taken; changed files; validation commands/environment/results; memory/disk impact where relevant; remaining risks; contract updates; next package prerequisites. Reports may use headings and tables, but must keep secrets and sensitive host configuration out.
