# Telegram alert contract

Telegram is required for the first release. One bot and one configured destination chat are sufficient for the owner's 10–15 services. No inbound Telegram webhook, command handler, or polling bot is needed.

## Setup and content

The operator creates a bot and provisions its token using a service-readable secret file outside the repository. The administrator configures the destination chat ID, enables delivery, and requests a test message through the authenticated settings page. Never return the token to the browser; show only whether it is configured. Bot creation and determining the chat ID belong in the future operator runbook. A user must make the destination available to the bot before a private message can be delivered.

Use Telegram `sendMessage` over verified HTTPS to the fixed official API origin. Do not allow user-supplied API base URLs in production. The token is part of the API request path, so redact URLs as well as headers and errors. Use plain text, no parse mode, and disable link previews. The API provides error data including retry information. See the [Telegram Bot API](https://core.telegram.org/bots/api).

Messages include monitor name, down/recovered status, observation time with timezone, concise sanitized reason, incident ID, and a configured dashboard link. Never include full target URLs with query strings, private IPs by default, credentials, raw exception dumps, or response bodies. Limit each message to 3000 characters as a conservative application limit. Label incident duration as observed duration when there are coverage gaps.

## Delivery semantics

- Transactionally persist an incident transition and its outbox event together. Each `(incident, event kind)` has one unique logical event.
- Send down only after the configured failure threshold. Send recovery after the first subsequent success. No repeated reminders in version one.
- Coalesce events received within five seconds into a digest, preserving per-monitor identity and chronological down/recovery events. This makes a WireGuard outage visible without 15 immediate separate messages.
- One sender, at most one message per three seconds for the configured destination, including test messages and retries. This deliberately conservative rate is below the typical per-chat guidance in the [Telegram FAQ](https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this); Telegram can still impose other limits.
- In normal operation, submit an alert within 15 seconds of incident confirmation. Provider delivery time is outside the service's control. A long backlog visibly reports delivery delay.
- Use a ten-second total request deadline and a 64 KiB response limit. Persist success with returned message ID. Retry network failures, 5xx, and 429; honor `retry_after` and bounded exponential backoff with jitter (start 5 seconds, cap 5 minutes).
- Stop after ten attempts or six hours, whichever comes first; mark expired/failed. Treat permanent authentication/chat errors as configuration failures, disable repeated automatic attempts until settings are corrected, and show an actionable dashboard banner.
- Order events within an incident. If both down and recovery are pending, combine them into a dated resolved-incident digest; never send an apparently current down alert for an already recovered service. A recovery whose down event expired must say the earlier outage alert was not delivered.
- At-least-once delivery is the practical contract: a crash after Telegram accepts a message but before local acknowledgement can produce a duplicate. Include stable incident IDs; do not claim exactly-once delivery.

## Bounds and operational failures

Cap pending outbox storage at 1000 event rows and 2 MiB of serialized content; enforce both before insertion. Store compact event references/fields, not copied HTML. Once full, retain existing jobs, record subsequent suppressed-event counters in a bounded singleton health record, and continue recording incidents. When space becomes available, enqueue one dated suppression summary. Suppressed events remain inspectable through incident history; the dashboard must disclose that alerts were dropped.

Retain terminal delivery metadata for seven days, at most 2000 rows. Disabling delivery stops sends and marks pending jobs canceled; enabling it does not replay canceled history. Configuration changes must not silently redirect queued messages: cancel old-destination jobs with an audit record, then offer a new test. Rate-limit the test action to once per minute.

If persistence fails, show delivery/storage degradation and do not create an unbounded memory queue. Telegram failure never alters target status or stops checking. A completely failed host or network path cannot send alerts; an independent external watchdog is still needed to observe the monitor itself.
