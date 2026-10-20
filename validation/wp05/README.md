# WP-05 validation and visual review

Generated results, logs, screenshots, transfer manifests and archives are kept locally and ignored by Git. The committed runners regenerate these artifacts; evidence links below refer to local outputs.

Run from the repository root. The browser runner uses a real loopback Rust
HTTP/auth/SQLite fixture, ephemeral credentials and temporary state under
`target/`. It runs no scheduler, outbound probes or Telegram sender. No real
credentials, target queries or tokens are used in screenshots.

```powershell
pwsh -NoProfile -File validation/wp05/run-windows.ps1
cargo build --locked --offline --example gui-review
# NODE_PATH points to the bundled Node packages from load_workspace_dependencies.
node validation/wp05/browser-review.mjs
```

The browser runner defaults to installed Edge (`msedge`); set `MM_BROWSER_CHANNEL`
for another installed Playwright Chromium channel. The browser automation package
is a development-only tool, not a Cargo or production dependency. The fixture
requires an ephemeral `MM_GUI_REVIEW_PASSWORD` supplied by the runner; it is
never a public bootstrap path. Linux checks run through [run-linux.sh](run-linux.sh)
in the documented, isolated remote WP-05 workspace; [prepare-transfer.py](prepare-transfer.py)
archives only allowlisted compile inputs and controlled runners.

Evidence (37 browser checks; 92 Windows / 94 Fedora tests):

- [Combined machine-readable summary](results-summary.json).

- [Windows commands/results](evidence/windows/exits.tsv), [environment](evidence/windows/environment.txt), [tests](evidence/windows/test.log).
- [Fedora commands/results](evidence/fedora/exits.tsv), [environment](evidence/fedora/environment.txt), [tests](evidence/fedora/test.log).
- [Browser assertions and contrast measurements](evidence/browser/results.json).
- [Asset byte/gzip measurements](evidence/assets.json).
- [Transfer manifest](transfer-manifest.json) pins the normalized compile inputs used remotely.

## Screenshots

| Review | Desktop | Mobile (360 px) |
| --- | --- | --- |
| Light overview | [Desktop](evidence/browser/desktop-overview-light.png) | [Mobile](evidence/browser/mobile-overview-light.png) |
| Dark overview | [Desktop](evidence/browser/desktop-overview-dark.png) | [Mobile](evidence/browser/mobile-overview-dark.png) |
| Dark detail | [Desktop/table](evidence/browser/desktop-detail-dark.png) | [Mobile](evidence/browser/mobile-detail-dark.png) |
| Light detail | [Desktop](evidence/browser/desktop-detail-light.png) | [Mobile](evidence/browser/mobile-detail-light.png) |
| Dark edit form | [Desktop](evidence/browser/desktop-edit-dark.png) | [Mobile](evidence/browser/mobile-edit-dark.png) |
| Dark settings | [Desktop](evidence/browser/desktop-settings-dark.png) | [Mobile](evidence/browser/mobile-settings-dark.png) |
| Login | [Light](evidence/browser/desktop-login-light.png) | [Light](evidence/browser/mobile-login-light.png), [dark](evidence/browser/mobile-login-dark.png) |
| Incidents | [Desktop](evidence/browser/desktop-incidents-dark.png) | Responsive scrollable table, reviewed in detail |
| Delete confirmation | Shared modal | [Mobile](evidence/browser/mobile-delete-light.png) |
| Stale fetch | [Desktop](evidence/browser/desktop-stale-dark.png) | Same global warning component |
| Stale observation | [Desktop](evidence/browser/desktop-stale-observation-dark.png) | [Unknown detail](evidence/browser/mobile-detail-dark.png) |
| Operational faults | [Desktop](evidence/browser/desktop-operational-faults-dark.png) | Same stacked warning components |
| Empty | [Search with no matches](evidence/browser/desktop-no-matches-dark.png) | [No monitors](evidence/browser/mobile-empty-light.png) |
| Missing monitor/history | [Monitor](evidence/browser/desktop-missing-monitor-dark.png), [history error](evidence/browser/desktop-history-error-dark.png) | Same explicit unavailable states |
| Session expired | Global warning | [Mobile](evidence/browser/mobile-session-expired-light.png) |
| Login errors | [Generic](evidence/browser/desktop-login-error-light.png), [throttled](evidence/browser/desktop-login-throttled-light.png) | Same login status component |
| Form errors | [403](evidence/browser/desktop-form-error-403-dark.png), [409](evidence/browser/desktop-form-error-409-dark.png), [413](evidence/browser/desktop-form-error-413-dark.png), [422](evidence/browser/desktop-form-error-422-dark.png), [429](evidence/browser/desktop-form-error-429-dark.png), [503](evidence/browser/desktop-form-error-503-dark.png) | Same modal error component |

## Accessibility review

Visible focus, semantic labels/headings, named controls, status text/icons,
local-timezone timestamps, native dialogs with explicit Tab/Shift+Tab containment,
Escape/focus return, a skip link, reduced-motion CSS and keyboard-operable chart
values are implemented. History has a table alternative. Automatic polls announce
status transitions only; changing form values does not trigger announcements.

Browser checks exercise modal keyboard traversal and focus return, timestamped
values, system theme changes, 360 px page overflow, inert hostile names and actual
contrast tokens against page/card/selected backgrounds in both themes. Input
borders and focus contrast meet 3:1; normal text/status/button combinations meet
4.5:1 in the measured palette. Small timeline marks have a table/visible-text
alternative rather than conveying status by color alone.

Screenshots were visually inspected for layout, hatching, focus, wrapping and
mobile stacking. Horizontally scrollable incident/history tables are intentional;
the page itself must not overflow. This is scoped review evidence, not a complete
WCAG audit. Real screen-reader use, Firefox/Safari and physical mobile devices
remain independent QA work.

Faults, empty data and expired-session responses are injected only in the review
runner; normal flows use the real API. The status fixture is synthetic controlled
SQLite data, not uptime or production resource evidence. The application never
ships demonstration observations.

## WP05-R1/R2/R3 correction evidence

Run the additional maintained regression suite after building the fixture:

```powershell
node validation/wp05/refresh-regressions.mjs
```

It uses the same development-only Playwright package and fixture. Node's built-in
`node:sqlite` edits controlled temporary incident/receipt records; no dependency
is added to the app. The test runs no probes or sender and never touches production
state. Timer and visibility controls drive the production polling paths. Delayed
history checks use the real protected server API, including a real 6.2-second
cache-age reproduction and exact full 90-day duration assertions.

Current evidence: **37 original + 17 focused browser checks**, **92 Windows / 94
Fedora tests**, with all formatting/clippy/release/lifecycle exits 0. One existing
Fedora namespace-only ping test remains ignored. Package acceptance still needs
independent re-review of the original findings.

- [Focused assertions](evidence/corrections/results.json), [combined results](results-summary.json).
- [Current Windows checks](evidence/corrections/windows/exits.tsv), [tests](evidence/corrections/windows/test.log).
- [Current Fedora checks](evidence/corrections/fedora/exits.tsv), [tests](evidence/corrections/fedora/test.log).
- [Current asset measurements](evidence/corrections/assets.json).
- [Initial transfer manifest](evidence/corrections/baseline-transfer-manifest.json) and [current compile inputs](transfer-manifest.json).

| Regression screenshot | Evidence |
| --- | --- |
| R1 desktop range focus with paused status | [Desktop](evidence/corrections/R1-desktop-focused-range-paused.png) |
| R1 mobile heartbeat focus and current status/warnings | [360 px](evidence/corrections/R1-mobile-focused-heartbeat-paused.png) |
| R2 real delayed-summary 90-day range | [Full 90 days](evidence/corrections/R2-real-90-day-delayed-summary.png) |
| R3 initial empty page gains an incident and pending delivery | [New incident](evidence/corrections/R3-new-incident-after-empty.png) |
| R3 recovery and delivered receipt appear on the next poll | [Updated snapshot](evidence/corrections/R3-recovery-delivery-refreshed.png) |
| R3 older cursor retained with explicit freshness/actions | [Older page](evidence/corrections/R3-older-page-retained.png) |
| R3 failed read preserves a stale snapshot with retry | [Stale snapshot](evidence/corrections/R3-stale-incident-snapshot.png) |

To preserve initial logs, the Windows runner accepts `MM_WP05_EVIDENCE` as a
project-relative evidence folder; correction checks used
`validation/wp05/evidence/corrections/windows`. Linux correction checks use the
new isolated `validation/wp05/corrections-20261018` directory under the already
approved Fedora project root, preserving the original WP-05 workspace.
