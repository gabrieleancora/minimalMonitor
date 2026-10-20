# Telegram alert contract

Telegram is required for the first release. The owner confirmed one destination channel containing the owner and other bots (D-20) and shares the sending bot with other applications (D-22). No inbound Telegram webhook, command handler, or polling bot is needed. Optional [Bark fallback](bark-fallback.md) is planned for version two; it never replaces Telegram.

WP-04 implements this contract with the existing Hyper/rustls dependencies
(D-24). The fixed official hostname uses public trust roots and rejects the whole
DNS answer set on special/private addresses or more than eight answers; at most
two numeric addresses may be tried before POST. There is one POST per admitted
attempt, no redirect, compression or proxy environment support, a ten-second
total deadline, 32 KiB response-header/parser bounds and a 64 KiB body bound.
An observed HTTP 429 retains its numeric Retry-After even if the body stalls or
fails; valid Bot API retry_after can extend the wait. All exposed errors are fixed
categories; provider descriptions are discarded.

Messages count UTF-16 units against the conservative 3000-character ceiling,
include explicit UTC dates, stable incident IDs, snapshot monitor names and fixed
probe-error categories, and omit target addresses/URLs. Five-second event windows
are packed into bounded messages; a pending resolved down incorporates its
recovery even outside that window, sharing one receipt. Acknowledged down delivery
is retained on the incident independently of seven-day receipt metadata.

## Setup and content

The operator provisions the existing bot's token using a service-readable secret file outside the repository; a dedicated replacement bot is not required. The administrator configures the destination channel's chat ID, enables delivery, and requests a test message through the authenticated settings page. Never return the token to the browser; show only whether it is configured. Bot creation for new installations and determining the chat ID belong in the future operator runbook. Add the bot to the channel with administrator posting permission (`can_post_messages`), then verify a controlled test; other bots require no integration. See [Telegram administrator rights](https://core.telegram.org/bots/api#chatadministratorrights).

Use Telegram `sendMessage` over verified HTTPS to the fixed official API origin. Do not allow user-supplied API base URLs in production. The token is part of the API request path, so redact URLs as well as headers and errors. Use plain text, no parse mode, and disable link previews. The API provides error data including retry information. See the [Telegram Bot API](https://core.telegram.org/bots/api).

Messages include monitor name, down/recovered status, observation time with timezone, concise sanitized reason, incident ID, and a configured dashboard link. Never include full target URLs with query strings, private IPs by default, credentials, raw exception dumps, or response bodies. Limit each message to 3000 characters as a conservative application limit. Label incident duration as observed duration when there are coverage gaps.

## Delivery semantics

- Transactionally persist an incident transition and its outbox event together. Each `(incident, event kind)` has one unique logical event.
- Send down only after the configured failure threshold. Send recovery after the first subsequent success. No repeated reminders in version one.
- Coalesce events received within five seconds into a digest, preserving per-monitor identity and chronological down/recovery events. This makes a WireGuard outage visible without 15 immediate separate messages.
- One sender, at most one message per three seconds for the configured destination, including test messages and retries. This deliberately conservative rate is below the typical per-chat guidance in the [Telegram FAQ](https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this); Telegram can still impose other limits.
- In normal operation, submit an alert within 15 seconds of incident confirmation. Provider delivery time is outside the service's control. A long backlog visibly reports delivery delay.
- Use a ten-second total request deadline and a 64 KiB response limit. Persist success with returned message ID. Retry network failures, 5xx, and 429 after 60 seconds from the failed attempt's completion. For 429, wait at least `max(60 seconds, retry_after)`; never cap a longer provider-requested wait. This replaces the earlier five-second exponential-backoff proposal (D-22). Queue pacing may delay a due retry further.
- Persist a bot-wide not-before timestamp on 429. Apply it to all local sends, including new alerts, retries and test messages, and preserve it across restart. A cooldown does not consume an attempt; count actual requests and enforce the existing expiry. The local rate limit cannot coordinate other applications using the same bot. See Telegram's [retry_after contract](https://core.telegram.org/bots/api#responseparameters).
- Stop after ten attempts or six hours, whichever comes first; mark expired/failed. Treat permanent authentication/chat errors as configuration failures, disable repeated automatic attempts until settings are corrected, and show an actionable dashboard banner.
- Order events within an incident. If both down and recovery are pending, combine them into a dated resolved-incident digest; never send an apparently current down alert for an already recovered service. A recovery whose down event expired must say the earlier outage alert was not delivered.
- At-least-once delivery is the practical contract: a crash after Telegram accepts a message but before local acknowledgement can produce a duplicate. Include stable incident IDs; do not claim exactly-once delivery.

In version two, a failed Telegram attempt makes an enabled/configured Bark fallback eligible immediately. Bark success or failure does not acknowledge the Telegram job, reset its retry clock or block its next due retry. Without Bark, version one follows the same Telegram retry schedule. Permanent authentication/chat failures retain the stop-until-corrected rule above; the one-minute automatic retry applies to transient failures.

## Bounds and operational failures

Cap pending outbox storage at 1000 event rows and 2 MiB of serialized content; enforce both before insertion. Store compact event references/fields, not copied HTML. Once full, retain existing jobs, record subsequent suppressed-event counters in a bounded singleton health record, and continue recording incidents. When space becomes available, enqueue one dated suppression summary. Suppressed events remain inspectable through incident history; the dashboard must disclose that alerts were dropped.

Retain terminal delivery metadata for seven days, at most 2000 rows. Disabling delivery stops sends and marks pending jobs canceled; enabling it does not replay canceled history. Configuration changes must not silently redirect queued messages: cancel old-destination jobs with an audit record, then offer a new test. Rate-limit the test action to once per minute.

If persistence fails, show delivery/storage degradation and do not create an unbounded memory queue. Telegram failure never alters target status or stops checking. A completely failed host or network path cannot send alerts; an independent external watchdog is still needed to observe the monitor itself.

A storage timeout can leave the outcome of a claim uncertain even before transport starts. Reconcile such claims with bounded state on the running sender; do not leave an outstanding request blocking all later alerts until restart. Keep delivery/storage degradation visible until reconciliation, preserving single-sender admission, event ordering, attempt/expiry limits and provider cooldowns. Validate lost claim replies separately from failed delivery acknowledgements (WP04-R1).

The WP04-R1 correction retains one notification permit while awaiting an uncertain
claim's reconciliation. The sender cannot enter transport without a successful
claim reply. Its next step submits `ReconcileUnsubmittedClaim` on the same SQLite
worker, behind the earlier claim: either that claim did not commit, or its
still-sending rows return to pending with the unsent reservation refunded and
expiry enforced. Canceled/terminal jobs remain terminal. The existing request
sequence is acknowledged without changing provider cooldown or pacing. This is
idempotent even when the reconciliation reply is also lost. Delivery health
stays degraded until a reconciliation reply confirms success. Only the same
admitted sender, before transport, may use this operation. Requests that may
already have reached transport retain the existing Finish/restart semantics.
