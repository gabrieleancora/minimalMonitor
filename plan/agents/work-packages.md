# Assignable work packages

WP-00 was assigned by the owner on 2026-10-06, retested on Fedora on 2026-10-09, and completed through the owner-authorized Hyper follow-up on 2026-10-10. Its [report](reports/WP-00.md) records passing Windows/Linux feasibility, bounded HTTP parsing, narrowly authorized non-root Linux ping and capped footprint evidence. The owner separately assigned WP-01 and WP-02 on 2026-10-10; their [foundation](reports/WP-01.md) and [security](reports/WP-02.md) reports record completed scoped implementation and Windows/Fedora evidence. WP-03 was assigned on 2026-10-12; its [monitoring report](reports/WP-03.md) records completed scoped implementation and Windows/Fedora evidence. The owner assigned WP-04 on 2026-10-14; its notification implementation and platform evidence are recorded in [WP-04](reports/WP-04.md). The owner assigned WP-05 on 2026-10-18; its [GUI handoff](reports/WP-05.md) records implementation and scoped Windows/Fedora/browser evidence, accepted after independent re-review on 2026-10-20. WP-06–WP-07 remain unassigned and not started. The owner selects an agent and gives the relevant package ID. This is an ordered implementation plan, not authorization to start a subsequent package.

| ID | Suggested role | Dependencies | Deliverable / completion gate |
| --- | --- | --- | --- |
| WP-00 | Backend + DevOps feasibility | Owner calls agent; Linux baseline needed for platform evidence | Minimal measured feasibility for candidate Rust stack, bounded password hashing, SQLite, verified HTTPS and non-root IPv4/IPv6 ping. Prove Windows ping strategy. Report footprint and packaging limits before feature expansion. |
| WP-01 | Backend foundation | WP-00 | Project layout/toolchain, config validation, worker bounds, process ownership, schema/migrations, health and error model; Windows/Linux build checks. |
| WP-02 | Backend security | WP-01 | Local admin bootstrap/reset, password/session/CSRF lifecycle, route protection, input/concurrency limits, destination policy and secret redaction; V-03 and relevant V-06 evidence. |
| WP-03 | Backend monitoring | WP-01, destination policy from WP-02; D-18/D-19/D-21 owner decisions | Extend policy with the bounded all-TCP flag (D-21), then HTTP/TCP/ping adapters, normal/confirmation scheduler, revision-safe transitions, incidents, coverage and aggregation; deterministic V-04/V-05 and policy evidence, eight-answer resolution limit, 180-day incidents without a row-count cap. |
| WP-04 | Backend notifications | WP-02, WP-03 incident contract, D-22 | Telegram secret integration, transactional outbox, digest/order/expiry limits, 60-second transient retries, durable shared-bot 429 cooldown and visible health; V-11 evidence. Bark is deferred. |
| WP-05 | GUI + API integration | WP-02, WP-03; WP-04 for alert settings completion | Responsive original Kuma-inspired GUI, themes, forms, charts/coverage, stale states, account and Telegram settings; V-02 evidence/screenshots. |
| WP-06 | DevOps | WP-00/WP-01, D-14/D-20 and production environment record; completed app for final integration | Debian 12/glibc 2.36-compatible build/runtime verification, current resource headroom, release packaging, service hardening, narrow ping permission, existing Nginx/TLS integration, NAS snapshot-pull backup preparation and restore/upgrade docs; V-09/V-10 evidence. |
| WP-07 | QA/security | WP-02 through WP-06 | Cross-component fault/security tests, 24-hour resource soak, retention and disk/backup/upgrade limits, restore rehearsal; complete acceptance report and release blockers. |

## WP-05 review gate (closed 2026-10-20)

The [independent re-review](reports/WP-05-review.md#re-review-and-closure-2026-10-20) closes WP05-R1/R2/R3 after code review, passing formatting/clippy on both platforms, 92 Windows/94 Fedora tests and 37 original plus 17 focused browser checks. WP-05 is accepted for scoped handoff. WP-06 is ready for a separate owner assignment; WP-06/WP-07 remain unassigned and unstarted. Full release qualification remains outstanding.

## WP-04 review gate (closed 2026-10-18)

The [independent re-review](reports/WP-04-review.md#re-review-and-closure-2026-10-18) closes WP04-R1 after inspecting same-sender reconciliation and rerunning formatting/clippy and the full suites: 91 Windows/93 Fedora tests pass, including four deterministic reply-loss regressions. The [worker correction report](reports/WP-04.md#wp04-r1-correction-2026-10-18) retains release/lifecycle evidence and qualification limits.

WP-04 is accepted for its scoped handoff. WP-05 was subsequently assigned on 2026-10-18 and accepted after re-review on 2026-10-20. WP-06–WP-07 remain unassigned and unstarted. Full release qualification remains outstanding.

## WP-03 review gate (closed 2026-10-14)

WP-04 was subsequently assigned by the owner and implemented with scoped Windows
and actual Fedora validation; its review gate is now closed above. See [its report](reports/WP-04.md). The historical
WP-03 review gate below is preserved. WP-05–WP-07 remain separately assignable.

The [re-review](reports/WP-03-review.md#re-review-and-closure-2026-10-14) closes WP03-R1 (retention cleanup during reanchor) and WP03-R2 (non-reused monitor identity and stale requests), with code review and independent Windows/Fedora checks. WP-03 is accepted for its scoped handoff and WP-04 is ready for a separate owner assignment. This does not dispatch an agent or start another package.

## Second-version backlog (unassigned)

**V2-01 — Optional Bark fallback:** after the Telegram/outbox and GUI contracts are implemented, add the [Bark fallback](../design/bark-fallback.md), provider-specific state/settings, migration, security and acceptance evidence. Telegram remains primary and still retries after Bark delivery. Reuse existing resource bounds and the owner's server. Confirm endpoint/version and secret provisioning before integration. This is a future assignment, not authorization to implement it or a WP-03/WP-04 release gate.

## Assignment prompts

**WP-00:** "Act as the implementation feasibility agent for WP-00. Read the plan, prove that the proposed stack and non-root ping strategy work within the stated resource constraints, and write the evidence report. Keep the prototype minimal and report failed targets before expanding scope."

**WP-01–WP-04:** "Act as the Rust backend agent for [package ID]. Read its dependencies and the linked design/security contracts. Implement only this assigned scope, validate its acceptance criteria, and write a report under plan/agents/reports. Do not weaken confirmed requirements to make tests pass."

**WP-05:** "Act as the GUI agent for WP-05. Use the original Uptime Kuma inspired design specification, small local assets and the agreed API. Implement light/dark/system modes and all error/stale states, then supply accessible desktop/mobile review evidence."

**WP-06:** "Act as the DevOps agent for WP-06. Integrate with the existing Nginx and WireGuard arrangement, prepare and test deployment/recovery artifacts, and record resource overhead. Preserve other services; production changes require an explicit assignment covering those changes."

**WP-07:** "Act as the QA/security agent for WP-07. Validate the release against plan/validation/acceptance.md using controlled test targets. Record measured memory and disk behavior, failures and release blockers without claiming tests that were not run."

## Definition of done

A work package has its required implementation/artifacts, relevant checks and report, with design deviations reconciled. The full first release additionally requires all confirmed requirements, real Linux verification, secure login, all four protocol variants (HTTP, HTTPS, TCP, ping), Telegram and resource evidence within the firm disk limit. A smaller intermediate milestone must be labeled incomplete rather than treated as the requested finished software.
