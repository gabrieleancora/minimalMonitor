# GUI design specification

Use [Uptime Kuma](https://github.com/louislam/uptime-kuma) as inspiration for the monitor list, prominent status, compact heartbeat history and simple detail view. Create original styling and assets; this is not a request to reproduce its codebase or all its features.

## Layout

Desktop: compact header, approximately 260-pixel monitor sidebar, flexible detail/main pane. Mobile: overview list first, detail opens as a separate view with an obvious back action; never require a permanently squeezed two-column view.

```text
minimalMonitor                    [Theme: System] [Settings] [Log out]
---------------------------------------------------------------------
[Search monitors] [+ Add]  |  Overview: 12 Up / 1 Down / 2 Paused
                          |  Last updated: 14:32:15
All / Down / Paused        |  [Operational warning only when relevant]
                          |
● Photos        Up        |  Photos                         [Edit] [...]
● Home server   Down      |  UP                  Last checked 9 s ago
○ Backup        Paused    |  42 ms     99.9% sample uptime    100% coverage
                          |  |||||||||||||||||||||||||||||||||||||||||
                          |  Recent checks, with timestamped tooltips
                          |  Latency chart           [1h] [24h] [7d] [90d]
                          |  Incidents / Check details
```

The overview and selected-monitor detail share the same navigation shell. Status counts must add up to the visible filter's total; distinguish pending, unknown and paused from down. Every monitor row includes text status, last observation age and a compact recent history indicator. On the overview, emphasize current failures before historical percentages.

Live status, observation freshness and operational warnings must continue updating when a chart, heartbeat or other control has keyboard focus. Preserve focus and unsaved inputs without freezing the entire main pane. The newest incident page and its delivery state follow the bounded visible-tab refresh cadence; older-page navigation must retain the user's page and disclose any intentionally retained snapshot, with a refresh action. History ranges must remain valid at the rolling retention boundary even when the last summary timestamp is older than a few seconds. These existing freshness/history requirements are regression gates WP05-R1/R2/R3.

## Screens and flows

| Screen | Required content and behavior |
| --- | --- |
| Login | Username/password, show-password toggle, generic errors, clear throttling message; no registration link |
| Empty dashboard | Brief purpose statement and Add monitor action; no fake chart/sample uptime |
| Add/edit monitor | Type picker, type-specific target fields, interval/deadline, accepted HTTP statuses, threshold, alerts toggle; inline validation and save feedback |
| Monitor detail | Explicit status, fresh/stale timestamp, latency, availability and coverage, recent samples, incidents, pause/resume/edit/delete |
| Settings | Theme preference, password change, Telegram enable/chat/test, token configured indicator, retention information and service health |
| Incident list | Monitor, observed start/confirmation/end, reason, gaps, alert delivery status |
| Delete confirmation | Names the monitor and explains associated history deletion; cancel is easy |

Do not add a "check now" control in the first release: periodic scheduling is sufficient and avoids a second probe admission path. Target policy rejections explain how the operator can update permitted destinations without exposing unrelated network details.

Show the confirmed normal interval separately from the fixed 30-second wait
between pending-failure confirmation checks. Pending status shows the current
failure count and next check time. Explain that confirmation checks contribute
to sample-based availability; do not label the percentage time-based uptime.

## Themes

Three options: Light, Dark, System. Default follows the OS. Store the preference in browser local storage, apply before first paint through a CSP-compatible local asset, and fall back gracefully when storage is unavailable. Login follows the same appearance preference. All controls and charts switch together.

Proposed palette directions, subject to accessibility verification:

| Token | Light | Dark |
| --- | --- | --- |
| Page | `#F4F6F8` | `#111827` |
| Surface | `#FFFFFF` | `#1F2937` |
| Main text | `#111827` | `#F9FAFB` |
| Secondary text | `#4B5563` | `#D1D5DB` |
| Accent | `#047857` | `#34D399` |

Use separate colors plus icons/text for up (green), pending (amber), down (red), unknown (neutral) and paused (muted). Status colors and filled-button foregrounds must be checked in actual combinations; these proposals are not proof of contrast compliance.

## Usability and performance

Use system fonts, consistent spacing, modest rounded cards and minimal animation. Accessible keyboard navigation, visible focus, semantic headings/form labels, and screen-reader names are required. Target WCAG AA contrast: 4.5:1 normal text, 3:1 large text and meaningful nontext controls. Do not announce every poll to assistive technology; announce meaningful status changes politely. Respect reduced motion.

Charts have a textual summary/table alternative and timestamped values available by keyboard. Unknown gaps remain visibly empty/hatched rather than connected green history. Timeline bars and chart ranges state whether they show individual probes or aggregated buckets. Zero data shows N/A. Display times in browser local timezone with its label; persist UTC.

Initial compressed page assets should total <= 250 KiB excluding API data; no external fonts, chart CDN or frontend runtime downloads. Qualify at 360-pixel mobile width and normal desktop width, in both themes. Future UI agents provide review screenshots and explain any design deviations in their handoff.

## WP-05 implementation and review

The [WP-05 report](../agents/reports/WP-05.md) and
[desktop/mobile evidence index](../../validation/wp05/README.md) record the
implemented local assets and review cases. Heartbeats show at most 12 actual
individual observations from the summary; no synthetic successes fill gaps.
Latency charts use the existing aggregated history API, label their scale and
granularity, and show missing/unknown buckets with hatching. Interactive values
have accessible names and visible feedback when activated, plus a history table.

Monitor detail's incident section uses the API's global cursor pages, explicitly
labels pages without matching incidents, and supports moving through older pages.
Historical transport reasons absent from persistence are not invented. Receipt
absence is unconfirmed delivery. This does not change incident or receipt retention.

### Review corrections supplied

WP05-R1/R2/R3 have [implementation/regression evidence](../agents/reports/WP-05.md#wp05-r1r2r3-corrections-2026-10-18) and were closed by [independent re-review on 2026-10-20](../agents/reports/WP-05-review.md#re-review-and-closure-2026-10-20). Live displays
update in place while range/heartbeat controls retain focus. Unsaved settings and
modal forms remain intact. Relative history ranges use a server-anchored full
window; the newest incident page and receipts refresh together, while older pages
disclose a retained dated snapshot and offer same-page refresh. Failed reads show
stale snapshots explicitly. The original freshness requirements above remain the
acceptance criteria.
