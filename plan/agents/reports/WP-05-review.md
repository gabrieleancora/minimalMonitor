# WP-05 planning-agent review

Date: 2026-10-18. Verdict: **WP-05 acceptance is pending WP05-R1/R2/R3.** The implementation and plan updates fit the assigned GUI scope, but live display freshness and the 90-day range need correction before handoff.

## WP05-R1 — P2: focused controls indefinitely freeze visible monitor status

Location: [app.js](../../../assets/app.js), lines 189–191.

Every successful poll replaces `data`, updates the sidebar and clears the stale banner, but skips all main-pane rendering whenever any descendant control has focus. A click on a chart range or heartbeat leaves focus there indefinitely. Subsequent target transitions and operational faults therefore do not reach the main overview/detail until focus moves elsewhere or the page reloads. On mobile the independently refreshed sidebar is hidden.

Reproduced with the existing Rust browser fixture and real API: opened Photos (Up), selected `1h`, paused that monitor through its authenticated API, and invoked the production visibility-resume refresh path. The pause returned 200; after the successful summary response the sidebar said **Paused**, the detail still said **Up**, focus remained on `1h`, and the stale banner was hidden. This is ordinary control focus, not unsaved form editing.

Required correction: update live status, observation freshness and operational warnings while preserving keyboard focus, chart selection and unsaved form values. Do not defer the entire main pane indefinitely just because it contains the active element. Regression: leave focus on a history-range/heartbeat control, change target state or health in the controlled fixture, run successive polls, and assert consistent visible data without focus loss; include mobile where the sidebar is hidden. R-02; F-04/F-08; V-02.

## WP05-R2 — P2: 90-day history fails when the cached summary is over five seconds old

Location: [app.js](../../../assets/app.js), lines 105–108; compare the existing rolling retention validation in [storage monitoring](../../../src/storage/monitoring.rs), lines 832–835.

History uses the previous summary's `server_time_ms` as its end and computes `start = end - range + 5000`. The server validates the oldest permissible start against its current time. At the 90-day limit, a summary older than five seconds therefore produces an out-of-retention request, even though the user selected a supported range. Polls occur every 15 seconds, and a focused range button can keep an earlier chart visible longer (R1).

Reproduced without intercepting responses: loaded a detail page, waited 6.2 seconds after requests settled, and clicked `90d`. The real API returned **422**; the GUI displayed “History unavailable” followed by irrelevant private-destination grant advice. The browser suite exercises `1h`, but misses this rolling-boundary case.

Required correction: construct a supported history window from sufficiently current server time, accounting for request delay at the retention boundary. Preserve the confirmed 90-day retention and API bounds; do not arbitrarily drop the oldest portion to hide the error. Regression: select 90 days at different ages of the cached summary, including after hidden-tab resume, and confirm a valid labelled window and successful response. R-02; F-05; V-02/V-04.

## WP05-R3 — P2: the Incidents page never refreshes its incident or delivery data

Location: [app.js](../../../assets/app.js), line 191, the incident branch gated by `!settingsLoaded`.

After its first successful load, `/incidents` sets `settingsLoaded=true`; all later summary polls skip `loadIncidents`. New incidents, recoveries and updated delivery receipts cannot appear automatically. An empty or short newest page has no incident-refresh control, and the page has no snapshot timestamp or stale explanation. This persists even when no control has focus, independently of R1.

Reproduced by counting real requests in the browser: initial page load fetched `/api/v1/incidents` once; two subsequent successful visibility-resume summary refreshes left that count at **one**. Code inspection confirms the same branch is used by timer refreshes and excludes both incidents and receipts after initialization.

Required correction: refresh the newest incident page and its delivery state on the bounded visible-tab cadence. Preserve explicit older-page navigation without silently jumping the user to the newest page; disclose freshness and provide a refresh action where an older snapshot is intentionally retained. Regression: keep the page open through a new incident/recovery and receipt update, including an initially empty list; verify updates, page selection, hidden-tab suspension and bounded requests. R-02/R-11; F-05/F-11; V-02/V-11 presentation.

## Independent validation

| Check | Windows | Existing isolated Fedora WP-05 checkout |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | Pass |
| `cargo clippy --locked --all-targets --offline -- -D warnings` | Pass | Pass |
| `cargo test --locked --offline` | 92 passed; zero failed/ignored | 94 passed; zero failed; one existing namespace-only ping test ignored |

All 48 selected source/config/runner files matched the normalized transfer manifest locally and remotely; manifest SHA-256 `a3974ff3231e255f90250d5eae47ad873d947e7ebf510b109c0e4bef65603d24`. Used installed stable toolchains, disabled rustup auto-install and retained project-local dependency caches and isolated target/temp paths. No source transfer or host configuration change was necessary.

Built the existing `gui-review` example and reran the supplied Playwright/Edge runner: **all 37 browser checks passed**, with zero captured JavaScript/CSP errors. Only its evidence destination was changed in memory to `target/wp05-review/browser`, preserving the worker's screenshots/results. Inspected representative desktop dark overview, mobile light detail and light login screenshots. Layout, themes and basic interactions are consistent with the requested original Kuma-inspired design.

Performed the additional read/interaction checks above against a separate instance of the existing fixture, using ephemeral generated credentials and temporary state. The fixture runs no probes, scheduler or Telegram sender; the pause affects only a synthetic local monitor. No application code or maintained regression tests were changed. The focused stale-detail screenshot is in `target/wp05-review/focused-stale-detail.png`; the precise observations are retained above so the report does not depend on ignored build artifacts.

The worker's release/lifecycle evidence was inspected, not independently rerun. No new namespace permission, screen-reader, Firefox/Safari, physical device, live Telegram, Debian 12 or resource-soak qualification is claimed. Documentation links and whitespace checks were repeated after these review updates.

## Plan assessment and handoff

D-25's embedded assets and bounded 12-observation preview are relevant, preserve the existing security/response bounds and avoid new runtime infrastructure. The report appropriately separates controlled GUI evidence from production qualification. Global incident pagination and explicit unconfirmed receipt absence are documented compromises within the existing API. The failing refresh behavior is not an accepted design deviation.

Updated the index, work-package gate, worker report notice, D-25, GUI contract and acceptance criteria to reflect the three findings. WP-05 remains pending corrections and independent re-review; no later package or agent was started. No commit, push or production action was performed.

## Implementation follow-up supplied (2026-10-18)

The owner assigned the GUI agent to apply these findings. [Corrections and maintained regression evidence](WP-05.md#wp05-r1r2r3-corrections-2026-10-18) are now supplied: keyed live display updates preserving focus/forms (R1), a full server-anchored relative history window (R2), and newest incident/receipt polling with explicit older snapshots and retry freshness (R3). The worker reran 92 Windows/94 Fedora tests, formatting/clippy/release/lifecycle checks, the original 37 browser checks and 17 focused review regressions. Original findings and review observations above are retained. **This implementation follow-up is not independent re-review or acceptance closure.**
