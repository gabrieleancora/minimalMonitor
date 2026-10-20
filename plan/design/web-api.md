# Web and API contract

Route names are proposed contracts for frontend/backend coordination. Version the JSON API as `/api/v1`; server-rendered pages may call it through small JavaScript modules. All monitoring data is private.

WP-01 established the two local health routes. WP-02 adds the login, logout,
account-session and password-change routes plus central protection for every
remaining path, including unknown/pending routes. Unauthenticated pages redirect
to login; API requests receive 401. Authenticated pending feature routes return
a bounded 501 without inventing monitoring data. WP-03 now supplies the monitoring
operations below; WP-04 adds notifications and WP-05 supplies the GUI. New feature routes must be assembled inside the
protected router before its guard is applied and extend the private-route tests.
Liveness/readiness use socket locality alone. Nginx must exclude both paths;
readiness now requires storage-worker progress/admission and scheduler progress.

## Pages and operations

WP-03 implements the monitoring JSON operations below within the existing guard.
WP-05 implements guarded dashboard/detail/settings/incident HTML shells; WP-04 implements Telegram settings/sends.
Create uses the monitoring fields, with optional `Idempotency-Key` (required for
automatic retries); edits use `{revision, config}`, pause/resume `{revision}`, and
delete `{revision, confirm_history_deletion:true}`. Revisions advance on schedule
mutations. Unknown/duplicate fields, unsafe targets and excess admission fail
without a commit. History accepts integer `start_ms`/`end_ms` or an exclusive integer
`range_ms`, and a retained 90-day window; incident cursors accept only integer `before`/`limit`. Recent
history clips exact raw slot counts; older windows disclose partial hourly
boundaries. Summary includes current status/streak, next due, system faults,
scheduler health and dropped/persistence counters. Responses enforce 256 KiB.

Monitor IDs are opaque signed integers. Use the returned ID in every route and
reference; do not assume positivity, ordering or reuse. Migration 3 retains
legacy positive IDs and allocates new IDs in a durable negative range. After
deletion, a retained create key returns 409 rather than another monitor; stale
mutations against the deleted ID return 404. The request/revision shapes and
100-key/24-hour bounds are unchanged.

| Route / operation | Authentication | Contract |
| --- | --- | --- |
| `GET /login` | Public | Login form and CSRF bootstrap; no monitor data |
| `POST /auth/login` | Public + pre-auth CSRF/origin checks | Bounded credentials; generic failure; rotate into authenticated session |
| `POST /auth/logout` | Session + CSRF | Revoke session and clear cookie |
| `GET /api/v1/account/session` | Session | Return current CSRF token only; no session token/hash or account credentials |
| `GET /` | Session | Dashboard shell and initial bounded summary |
| `GET /monitors/{id}` | Session | Detail page |
| `GET /settings` | Session | Account, appearance, Telegram status, operational health |
| `GET /api/v1/summary` | Session | At most 50 current monitor summaries, global health and server timestamp |
| `POST /api/v1/monitors` | Session + CSRF | Validate and persist configuration; 201 after commit |
| `PATCH /api/v1/monitors/{id}` | Session + CSRF | Edit with expected revision; 409 on stale edit |
| `POST /api/v1/monitors/{id}/pause` or `/resume` | Session + CSRF | Apply state transition and scheduler admission rules |
| `DELETE /api/v1/monitors/{id}` | Session + CSRF | Confirmed deletion; 202 while bounded cleanup finishes |
| `GET /api/v1/monitors/{id}/history` | Session | Validated window and <= 300 chart buckets; no unlimited raw export |
| `GET /api/v1/incidents` | Session | Cursor pagination; default 25, maximum 100 per page |
| `POST /api/v1/account/password` | Session + CSRF + current password | Hash under bounded work admission; invalidate all sessions after success |
| `GET/PATCH /api/v1/settings/telegram` | Session; CSRF for PATCH | Enable/chat configuration, token-present flag, delivery status; no token readback |
| `POST /api/v1/settings/telegram/test` | Session + CSRF | Queue one rate-limited test; return 202 and delivery status identifier |
| `GET /api/v1/notifications` | Session | Recent bounded delivery states; default 25, maximum 100 |
| `GET /health/live` | Local only | Minimal process liveness; proxy must not publish |
| `GET /health/ready` | Local only | Database usable and workers progressing; no target data |

Appearance preference is local to the browser and does not need a database write. Ordinary pages redirect unauthenticated users to login; JSON requests receive 401. No status/settings endpoint is anonymously exposed. Serve public login assets without embedding configuration or secrets.

### WP-05 presentation integration

`GET /incidents` is an authenticated incident-history shell alongside the existing
page routes. Embedded local `/assets/app.css`, `/assets/theme.js`, `/assets/app.js`
and `/assets/login.js` are public nonsensitive assets under the same canonical
Host/CSP boundary. No data or configuration is embedded in those assets.

Summary adds `recent`, an object keyed by the exact decimal monitor ID. Each
value contains at most 12 individual raw observations in chronological order:
`{scheduled_ms, observed_ms, outcome}`. The existing indexed storage worker reads
at most 50 × 12 rows, without exposing target/query/error text. The response
keeps the existing 256 KiB cap. Existing monitor and mutation shapes are unchanged.
Browser parsing preserves opaque integer identifiers outside JavaScript's safe
integer range; chat IDs remain canonical decimal strings.

The client polls one summary per visible tab every 15 seconds plus <=750 ms
jitter, backing off to 60 seconds plus jitter after failures; hidden tabs stop
polling and refresh on return. Detail history reads are serialized; focused range controls retain their DOM
identity, and their handlers prevent overlapping reads. Forms retain revisions and errors without polling
over their inputs. Mutations are never automatically retried. A create retry
reuses its idempotency key while inputs are unchanged. Password-change 401 is
checked against the session endpoint before classifying it as a wrong current
password or expired session.

### WP05-R1/R2/R3 correction contract

Relative history requests use `?range_ms=<duration>` exclusively; combining this
with `start_ms` or `end_ms`, duplicate/unknown fields, zero/negative values and
values above 90 days is invalid. The handler anchors both window endpoints and
storage validation to one current server timestamp. The response labels those
exact endpoints. Explicit UTC start/end requests retain their existing validation,
including rejection outside the rolling retention window. No client margin trims
the oldest part of a requested range and retention remains 90 days.

Main-pane status, observation freshness, health warnings and history update
regardless of control focus. Keyed DOM updates preserve surviving control identity,
expanded disclosures and focus; mutation handlers receive current monitor revisions.
Settings forms and modal inputs are not replaced by polls.

Newest incident pages fetch one bounded 25-incident page and one bounded 100-receipt
page per completed visible-tab poll. A per-section in-flight guard covers both
requests through settlement. Older cursor pages retain their incident/receipt
snapshot across automatic polls, show its timestamp and explain the pause, and
provide explicit same-page refresh and newest-page actions. A failed incident or
receipt read preserves the previous complete snapshot with a stale notice and
retry action. Summary failure still shows the existing global stale banner.

## Validation and limits

WP-04 implements Telegram routes inside the existing authenticated/CSRF guard.
`GET /api/v1/settings/telegram` returns `{settings, token_present,
persistence_degraded}`. Settings contain revision, enabled, nullable chat ID,
destination revision, configuration-failed flag, cooldown, safe last error,
last success, pending count/oldest timestamp and dropped count. Chat IDs are
returned as canonical signed decimal **strings** to preserve every SQLite i64
value in browser JavaScript. PATCH accepts `{revision, enabled, chat_id}`;
chat ID may be null, a canonical decimal string (preferred for browsers), or an
exact JSON integer. Zero, overflow, noncanonical strings, unknown/duplicate fields,
tokens and custom origins are rejected. Enabling requires a configured token and
chat. Stale revision is 409. A valid corrective PATCH clears the permanent-error
latch; changing destination/enablement cancels old pending jobs and audits it.

The test POST has an empty body, returns `{id,state:"pending"}` with 202 and
enforces a persisted once-per-minute action limit (429/Retry-After:60). The sender
applies its usual pacing/cooldown to that job. Notifications GET accepts only
numeric `before`/`limit`, defaults to 25, caps at 100, and returns bounded receipt
metadata without payloads, token fingerprints, targets or raw provider errors.
Summary includes `telegram` settings/health, token presence, persistence
degradation and a delivery-delay flag after 15 seconds of pending age. Rendering
its banner/forms belongs to WP-05.

- Same-origin requests only; no permissive CORS. Mutations use JSON and a CSRF header; login form has equivalent protection.
- Default body limit 16 KiB, request-header budget 16 KiB, normal request timeout five seconds, and 16 concurrent web requests. Hash work has separate admission. Enforce limits at both proxy and application boundaries where applicable.
- Login and password-change JSON have a stricter 1024-byte encoded body limit.
  Unknown fields, duplicate credential fields and invalid JSON are rejected with
  safe errors. Login accepts `username`/`password`; password change accepts
  `current_password`/`new_password`. Successful login returns `csrf_token` and
  sets a new session cookie; logout/change return 204 after durable revocation.
  `/login` contains a public minimal form and signed preauth token; its local
  `/assets/login.js` posts JSON and uses safe text for errors. Full GUI is WP-05.
- URL length <= 2048 bytes, host <= 253 characters, port 1–65535, monitor name <= 100 characters. Reject userinfo, fragments, unsupported schemes, control characters and ambiguous target parsing. Error text returned to clients <= 256 characters.
- Validate cadence/capacity using D-19's confirmation-aware admission rule,
  not just the normal interval. Return pending failure count and next due time
  to the GUI; the 30-second confirmation wait is fixed in this release. Browser
  monitor requests cannot modify D-21's operator-managed destination grants.
- Telegram chat ID is validated and stored in a type that preserves its full signed value. No arbitrary destination URL or token field in the browser API.
- API responses <= 256 KiB. History bounds: maximum 90-day window, declared granularity and coverage, no more than 300 points. Raw-detail views, if added later, require separate bounded pagination.
- No GET operation changes account, monitor or Telegram configuration. Session activity updates are internal bookkeeping, batched to avoid a write per poll.
- Use version/revision checks to prevent stale forms overwriting newer settings. For retryable create requests, client idempotency keys prevent duplicate monitors; retain at most 100 keys for 24 hours.

## Error shape and browser behavior

The [security contract](../operations/security.md) specifies canonical Host and
Origin, trusted proxy identity and cookie names. Current security routes accept
no query parameters or absolute-form request targets. Future history/pagination
routes must replace that query refusal with their own bounded validation.
Rate/busy responses carry Retry-After; no raw extractor/SQL/OS text is returned.

Return a stable error code, safe human-readable message, optional field errors, and request ID. Use 400/422 for malformed/invalid data, 401 unauthenticated, 403 CSRF/authorization, 404 missing, 409 revision/capacity conflicts, 413 oversized, 429 rate-limited, and 503 busy/degraded. Do not return stack traces, SQL, paths or full outbound URLs.

Poll summary once per 15 seconds per visible tab, with jitter and backoff to 60 seconds after failures. Pause automatic refresh in hidden tabs; refresh on return. One summary call serves the whole list. An authentication failure stops polling and shows login. Network failure retains existing data but adds a prominent stale timestamp; never preserve a green status without a stale indicator.
