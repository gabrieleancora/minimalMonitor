# Optional Bark fallback — version two

Status: planned, unassigned; not a first-release gate. D-22 confirms the owner's existing home-hosted Bark server and the requirement that Telegram remain primary. Detailed defaults below are proposed for the later implementation assignment; no generic notification framework or additional server deployment is needed.

## Delivery flow

1. Attempt Telegram using the [notification contract](notifications.md).
2. On failure, attempt Bark if enabled and configured. An active Telegram rate-limit cooldown also makes new events eligible for Bark without sending a forbidden Telegram request first. Skip Bark when disabled/unconfigured; determine availability through the bounded send itself, without a separate health probe.
3. Keep Telegram pending after Bark success or failure. Retry Telegram when due: 60 seconds from its failed attempt, or longer when `retry_after` requires it. Bark does not reset this clock. Permanent Telegram configuration errors still require correction before retrying.

Proposed small-scope defaults: one Bark endpoint and one iPhone/device key; at most one automatic Bark attempt per logical event, rather than another push on every Telegram retry. Use a five-second total deadline and 64 KiB response ceiling. Share the existing single notification-send slot, yield it between requests, and prioritize due Telegram retries; waiting for cooldowns never holds the slot. Pace Bark sends at most once per three seconds and reuse compact digests during a home-wide outage.

Record Telegram and Bark outcomes separately. Bark acceptance does not prove that the iPhone displayed the notification. Timeouts/crashes can leave acceptance uncertain, and stable incident IDs make possible duplicates recognizable. Do not claim exactly-once delivery. Tests and retries must not recreate an already recorded Bark attempt; persist a started/uncertain outcome before sending, accepting that a crash can lose this best-effort fallback.

Preserve chronological incident context independently for each provider. Never send an apparently current down message after recovery; coalesce pending down/recovery into a dated resolved summary. If Bark accepted a down message, or its acceptance is uncertain, also attempt a Bark recovery even when Telegram recovery succeeds. A failed Bark recovery stays visibly failed; the one-attempt fallback does not promise eventual delivery. Manually requested test messages test the selected provider only and never trigger cross-provider fallback.

## Configuration, security and bounds

- Reuse the owner's existing server. Operator configuration selects its HTTPS origin/base path; the browser may enable/disable delivery and request a test but must not accept arbitrary endpoint URLs or return credentials. Store the device key and any server authentication in secret files.
- Use JSON `POST /push` with `device_key`, title/body and the configured dashboard link, as supported by the [official Bark API](https://github.com/Finb/bark-server/blob/master/docs/API_V2.md). Do not put the key or message in a URL. Validate HTTP and application-level success against the installed server version; no critical/ringing alerts or remote images by default.
- Require verified TLS, no redirects, bounded DNS resolution and validated-address connection pinning. Apply the existing outbound policy, including explicit private grants and unconditional metadata denials, to the configured Bark destination. A notification endpoint is not exempt from SSRF controls. Redact credentials, request bodies and sensitive endpoint details from logs/errors.
- Reuse Telegram's sanitized content limits. Persist separate provider outcomes, next Telegram attempt and destination revisions with the logical event through a versioned migration when v2 is assigned. Keep the existing aggregate outbox limits (1000 pending event rows / 2 MiB content) and terminal metadata limits; do not duplicate unbounded queues or add a second process. Retain a bounded incident-level flag for Bark recovery routing when down delivery metadata has aged out.
- Disabling a provider cancels its pending work only. Endpoint/device changes cancel old-revision work rather than redirecting it. Re-enabling does not replay canceled history. Keep provider-specific health and test results visible; rate-limit each provider's manual test to once per minute. Dashboard/API additions and migration details belong to the v2 assignment.

Because Bark runs at home, a home power/router/WireGuard outage may also make it unreachable. It is useful for Telegram failure while the home path works, but is not an independent watchdog. Neither provider's failure changes target status or stops checks.

## Future assignment and acceptance

Before implementation, confirm the existing Bark server version, HTTPS endpoint, required authentication and device registration mechanism; provision secrets outside chat/reports. See the unassigned V2-01 entry in [work packages](../agents/work-packages.md). Keep the same memory/disk budgets and remeasure the finished feature.

Future acceptance must cover Telegram success without Bark; transient failure followed by Bark success and a still-pending Telegram retry; unavailable/disabled/unconfigured Bark; 429 with short/long `retry_after`, cooldown across restart and new events; permanent Telegram configuration failure; recovery and resolved digests on each provider; uncertain sends/restarts; bounded bursts/full queues; destination changes; TLS/policy/redaction; and visible provider-specific health. These are v2 checks, not claims of completed implementation.
