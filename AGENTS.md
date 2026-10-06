# minimalMonitor agent instructions

## Scope and roles

- Work within the owner's current assignment. The owner explicitly calls agents; do not spawn, dispatch, or message other agents unless the owner authorizes it.
- The planning agent maintains specifications and assignments only. It must not implement application code, run implementation prototypes, or deploy software.
- Development, GUI, DevOps, and QA agents may perform their assigned work. The planning-only restriction does not prevent an explicitly assigned implementation agent from coding.
- A work package describes future work; its presence is not authorization to start it. Do not automatically advance to another package.
- Carry out routine, reversible steps within an authorized assignment without repeatedly asking permission. Raise unresolved decisions only when they materially affect that work.

## Read before working

Start with these documents, then read the design and operations specifications relevant to the assignment:

1. [Planning index](plan/README.md)
2. [Requirements](plan/requirements.md) and [decision register](plan/decisions.md)
3. [Agent workflow and coding instructions](plan/agents/README.md)
4. [Work packages and dependencies](plan/agents/work-packages.md)
5. [Acceptance criteria](plan/validation/acceptance.md)

The owner's explicit instructions take precedence over repository guidance. Distinguish confirmed requirements from proposed defaults and open questions. If documents conflict, identify the discrepancy and reconcile the affected specifications; do not silently weaken a confirmed requirement.

## Product constraints

- Rust service, SQLite, authenticated web GUI, and x86_64 Linux production support; development takes place on Windows.
- First release: HTTP/HTTPS, TCP and ping checks, dashboard incidents, and Telegram down/recovery alerts for 10–15 services.
- Light, dark and system appearance modes with an original Uptime Kuma inspired interface.
- Preserve the conservative 256 MiB incremental RAM envelope and firm 2 GiB incremental disk limit. Detailed budgets and qualification scenarios live in the plan; estimates are not measured results.
- Integrate with the owner's existing Nginx and WireGuard arrangement. Preserve other sites, routing, and services.
- Keep the service small. Do not introduce additional infrastructure, a heavy frontend runtime, or deferred features without a scope decision.

## Repository organization and changes

- Keep specifications, design decisions, acceptance plans and handoff reports under `plan/`. Keep implementation code, tests, build outputs and executable deployment assets outside it.
- Inspect the current tree and Git status before editing. Preserve unrelated changes, including staged work; do not reset, overwrite or reformat them.
- Use the plan's requirement and work-package IDs in implementation reports. Keep detailed defaults in their authoritative specification instead of duplicating them here.
- Update affected contracts, acceptance criteria and the decision register together when a design changes. Do not mark work complete without supporting evidence.
- Do not initialize an application or install tooling merely because only planning files exist. Such work belongs to an implementation assignment.

## Implementation rules

- Follow the detailed [coding instructions](plan/agents/README.md). Prefer a small modular application, stable Rust, minimal dependency features, and a committed application lockfile.
- Bound concurrency, channels, pools, caches, request sizes, query results, logs, history and alert queues. Keep SQLite and password hashing off async runtime workers.
- Prefer safe Rust; isolate and document unavoidable platform-specific unsafe code. Ping must not require running the service as root or executing shell commands from target input.
- Enforce authentication, authorization, CSRF protection, input validation, verified TLS and outbound destination policy on the server. Private-network monitoring must use explicit operator policy.
- Keep credentials, Telegram tokens, cookies and sensitive URLs out of source, fixtures, logs, screenshots and reports. Use the planned secret-file mechanism.
- Treat missed observations as unknown coverage. Separate target status from storage/scheduler faults and Telegram delivery failures.
- Build and test Linux behavior explicitly; Windows success or cross-compilation alone is not production verification. Do not compile on the resource-constrained monitor host.

## Validation and handoff

- For documentation-only work, check links and consistency; do not create an application or run unrelated test suites.
- Once a Rust project exists, use its documented checks. Unless superseded by project-specific commands, run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked` for applicable Rust changes. Test relevant supported features and platforms; explain unavailable checks.
- Add meaningful tests for changed behavior, especially state transitions, persistence, resource bounds and security. Use controlled test targets; do not flood third-party services.
- Follow the acceptance plan for integration, security and resource qualification. Report actual commands, environment and results; never present unrun tests or unmeasured footprint targets as passed.
- After an assigned work package, write `plan/agents/reports/<package-id>.md` with scope, changed files, requirement coverage, validation evidence, deviations, remaining issues and next prerequisites.
- Deployment, publishing and production infrastructure changes must be covered by the owner's assignment. Preparing reviewable deployment artifacts does not itself authorize applying them to the host.

## Code review rules

Flag authentication bypasses, missing target-policy enforcement, unbounded resource use, secret leakage, fabricated uptime during observation gaps, lost or misordered alert transitions, and Windows-only assumptions in production paths. Evaluate deviations against the confirmed requirements and acceptance evidence, not merely whether the code builds.

## Folder exclusions
- Do not read, search, or modify `prompts/` fodler and its files.