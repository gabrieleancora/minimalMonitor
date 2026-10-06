# Web and API contract

Route names are proposed contracts for frontend/backend coordination. Version the JSON API as `/api/v1`; server-rendered pages may call it through small JavaScript modules. All monitoring data is private.

## Pages and operations

| Route / operation | Authentication | Contract |
| --- | --- | --- |
| `GET /login` | Public | Login form and CSRF bootstrap; no monitor data |
| `POST /auth/login` | Public + pre-auth CSRF/origin checks | Bounded credentials; generic failure; rotate into authenticated session |
| `POST /auth/logout` | Session + CSRF | Revoke session and clear cookie |
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

## Validation and limits

- Same-origin requests only; no permissive CORS. Mutations use JSON and a CSRF header; login form has equivalent protection.
- Default body limit 16 KiB, request-header budget 16 KiB, normal request timeout five seconds, and 16 concurrent web requests. Hash work has separate admission. Enforce limits at both proxy and application boundaries where applicable.
- URL length <= 2048 bytes, host <= 253 characters, port 1–65535, monitor name <= 100 characters. Reject userinfo, fragments, unsupported schemes, control characters and ambiguous target parsing. Error text returned to clients <= 256 characters.
- Telegram chat ID is validated and stored in a type that preserves its full signed value. No arbitrary destination URL or token field in the browser API.
- API responses <= 256 KiB. History bounds: maximum 90-day window, declared granularity and coverage, no more than 300 points. Raw-detail views, if added later, require separate bounded pagination.
- No GET operation changes account, monitor or Telegram configuration. Session activity updates are internal bookkeeping, batched to avoid a write per poll.
- Use version/revision checks to prevent stale forms overwriting newer settings. For retryable create requests, client idempotency keys prevent duplicate monitors; retain at most 100 keys for 24 hours.

## Error shape and browser behavior

Return a stable error code, safe human-readable message, optional field errors, and request ID. Use 400/422 for malformed/invalid data, 401 unauthenticated, 403 CSRF/authorization, 404 missing, 409 revision/capacity conflicts, 413 oversized, 429 rate-limited, and 503 busy/degraded. Do not return stack traces, SQL, paths or full outbound URLs.

Poll summary once per 15 seconds per visible tab, with jitter and backoff to 60 seconds after failures. Pause automatic refresh in hidden tabs; refresh on return. One summary call serves the whole list. An authentication failure stops polling and shows login. Network failure retains existing data but adds a prominent stale timestamp; never preserve a green status without a stale indicator.
