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
