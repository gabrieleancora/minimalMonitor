# Assignable work packages

All packages are unassigned and not started. The owner selects an agent and gives the relevant package ID. This is an ordered implementation plan, not an instruction to start coding now.

| ID | Suggested role | Dependencies | Deliverable / completion gate |
| --- | --- | --- | --- |
| WP-00 | Backend + DevOps feasibility | Owner calls agent; Linux baseline needed for platform evidence | Minimal measured feasibility for candidate Rust stack, bounded password hashing, SQLite, verified HTTPS and non-root IPv4/IPv6 ping. Prove Windows ping strategy. Report footprint and packaging limits before feature expansion. |
| WP-01 | Backend foundation | WP-00 | Project layout/toolchain, config validation, worker bounds, process ownership, schema/migrations, health and error model; Windows/Linux build checks. |
| WP-02 | Backend security | WP-01 | Local admin bootstrap/reset, password/session/CSRF lifecycle, route protection, input/concurrency limits, destination policy and secret redaction; V-03 and relevant V-06 evidence. |
| WP-03 | Backend monitoring | WP-01, destination policy from WP-02 | HTTP/TCP/ping adapters, scheduler, revision-safe transitions, incidents, coverage and history aggregation; deterministic V-04/V-05 evidence. |
| WP-04 | Backend notifications | WP-02, WP-03 incident contract | Telegram secret integration, transactional outbox, digest/order/retry/rate/expiry limits and visible health; V-11 evidence. |
| WP-05 | GUI + API integration | WP-02, WP-03; WP-04 for alert settings completion | Responsive original Kuma-inspired GUI, themes, forms, charts/coverage, stale states, account and Telegram settings; V-02 evidence/screenshots. |
| WP-06 | DevOps | WP-00 environment findings, WP-01; completed app for final integration | Linux release packaging, service hardening, narrow ping permission, scoped existing-Nginx integration, TLS, logs, backup/restore/upgrade docs and Windows/Linux CI; V-09/V-10 evidence. |
| WP-07 | QA/security | WP-02 through WP-06 | Cross-component fault/security tests, 24-hour resource soak, retention and disk/backup/upgrade limits, restore rehearsal; complete acceptance report and release blockers. |

## Assignment prompts

**WP-00:** "Act as the implementation feasibility agent for WP-00. Read the plan, prove that the proposed stack and non-root ping strategy work within the stated resource constraints, and write the evidence report. Keep the prototype minimal and report failed targets before expanding scope."

**WP-01–WP-04:** "Act as the Rust backend agent for [package ID]. Read its dependencies and the linked design/security contracts. Implement only this assigned scope, validate its acceptance criteria, and write a report under plan/agents/reports. Do not weaken confirmed requirements to make tests pass."

**WP-05:** "Act as the GUI agent for WP-05. Use the original Uptime Kuma inspired design specification, small local assets and the agreed API. Implement light/dark/system modes and all error/stale states, then supply accessible desktop/mobile review evidence."

**WP-06:** "Act as the DevOps agent for WP-06. Integrate with the existing Nginx and WireGuard arrangement, prepare and test deployment/recovery artifacts, and record resource overhead. Preserve other services; production changes require an explicit assignment covering those changes."

**WP-07:** "Act as the QA/security agent for WP-07. Validate the release against plan/validation/acceptance.md using controlled test targets. Record measured memory and disk behavior, failures and release blockers without claiming tests that were not run."

## Definition of done

A work package has its required implementation/artifacts, relevant checks and report, with design deviations reconciled. The full first release additionally requires all confirmed requirements, real Linux verification, secure login, all four protocol variants (HTTP, HTTPS, TCP, ping), Telegram and resource evidence within the firm disk limit. A smaller intermediate milestone must be labeled incomplete rather than treated as the requested finished software.
