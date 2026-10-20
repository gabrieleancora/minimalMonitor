# Internet-exposure security plan

Security is shared between the application, existing Nginx, the Linux host, and the hosting/network provider. WireGuard protects its tunnel; it does not authenticate users of an internet-facing dashboard.

## Threats and ownership

| Threat | Application responsibility | Operations responsibility |
| --- | --- | --- |
| Password guessing / login flood | Safe hashes, bounded hashing, generic errors, sessions, rate limits | Nginx login limits and connection/time limits |
| Request / slow-client flooding | Body/header/query limits, finite concurrency and queues | Nginx buffering, timeouts, rate/connection limits |
| Large volumetric DDoS | Remain bounded where traffic reaches the app | Provider/upstream mitigation; optional edge filtering |
| Target-driven SSRF / network abuse | Target validation and destination enforcement | Restrict egress to intended targets where practical |
| XSS, CSRF, injection | Escaping, CSRF, parameterized SQL, safe errors | TLS and response-header verification |
| Disk/log exhaustion | Retention, bounded data and sampled errors | Per-service log rotation and free-space observation |
| Compromised service | Minimal exposed functionality; no commands or plugins | Dedicated account, restricted filesystem and privileges |

Upstream mitigation matters because host-local request limits cannot recover a saturated network link. The deployment runbook should record the provider's protection/contact procedure, without promising DDoS immunity. [CISA guidance](https://www.cisa.gov/sites/default/files/publications/understanding-and-responding-to-ddos-attacks_508c.pdf).

## Authentication and sessions

WP-02 implements this baseline; [evidence](../agents/reports/WP-02.md) and
[operator commands](../../README.md#account-and-destination-security) describe
the intermediate build. No monitoring transports, Telegram sender, full GUI or
production proxy integration is supplied by this package.

- Bootstrap the only admin through a local interactive command; no default credentials, unauthenticated setup route, or open signup. Prompt securely rather than putting passwords in process arguments or logs.
- Local bootstrap/reset acquires exclusive process ownership with the service stopped. A reset revokes sessions before restart; it does not create a second concurrent database/scheduler owner.
- Use Argon2id with a starting minimum of 19 MiB, two iterations, one lane, unique random salt and stored parameters. Benchmark on the host and retain only one concurrent hash operation; reject excess work rather than queue indefinitely. This baseline comes from [OWASP password-storage guidance](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).
- Propose passwords of 15–128 characters, no mandatory composition rules, no silent truncation. Cap encoded credential input at 1024 bytes, username at 64 characters. Allow paste and password managers.
- Login errors do not reveal whether the username exists. Unknown users take equivalent bounded hash work. Apply rate limits before hashing.
- Use cryptographically random 256-bit opaque session tokens; store only token hashes. Cookie: `__Host-` prefix, Secure, HttpOnly, SameSite=Lax, Path=/, no Domain. No authentication tokens in local storage.
- Expire after two hours idle or 24 hours absolute, whichever first; cap five active sessions, revoking oldest when exceeded. Rotate token on login. Logout, local reset and password change revoke affected sessions immediately. Password change requires the current password and invalidates all sessions.
- Enforce authorization centrally for every private route. Use a production canonical origin/host allowlist; do not trust arbitrary Host or forwarded headers.
- Protect all mutations, including login/logout, with CSRF tokens and origin checks. Use a signed, short-lived pre-auth token/cookie flow without an unbounded anonymous session table. SameSite is an additional defense, not the sole control. [OWASP CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).

Implementation details: Argon2id v19 fixes m=19456 KiB, t=2, p=1, a random
16-byte salt and 32-byte output. Stored parameters are checked before verification
so corrupt data cannot request arbitrary hashing memory. Username normalization
trims outer whitespace and folds ASCII case; non-ASCII spelling is retained.
Password input is never truncated. Credential JSON bodies have a 1024-byte limit.
Only a successfully verified snapshot with the current credential generation can
create a session or change the password. All account/session writes recheck
storage admission inside the SQLite worker and acknowledge only a committed
transaction. Success audit metadata is bounded to 5000 rows/30 days; denials
increment an in-memory counter without durable per-request logs.

Preauth uses HMAC-SHA256 under a random per-process key over an issued timestamp
and random 256-bit nonce, expires at five minutes and requires the identical
cookie/header token plus canonical Origin. Restart invalidates outstanding
preauth tokens. A fresh session token and separate random CSRF binding follow
successful login; only the session token's SHA-256 is stored. Authenticated CSRF
is returned by the login response or private account-session endpoint.
Production cookies are `__Host-mm-session` and `__Host-mm-preauth`. Explicit
loopback HTTP development uses `mm-dev-session`/`mm-dev-preauth` without Secure;
this exception is never selected for production or an HTTPS development origin.
Cookies are HttpOnly/SameSite=Lax, Path=/, with no Domain in either mode.

Session activity is persisted at most once per minute. Idle expiry uses that
persisted checkpoint and can conservatively expire up to one minute before the
last in-memory request's two-hour anniversary; no expiry is extended beyond the
24-hour absolute limit. Clock rollback invalidates affected sessions. Expired
rows are pruned by bounded maintenance and before new sessions. Reset/password
change revoke all sessions atomically; login evicts oldest sessions before the
sixth insertion. Private routes fail closed if activity cannot be persisted.

## Application limits

Initial login limits: five attempts per minute per client IP with burst five, and six hash starts per minute globally with burst two. No permanent username lockout that an attacker can trigger. An attacker can still temporarily consume public login capacity; expose an operational metric and preserve local recovery.

Rate-limit maps have a 2048-entry cap and expiry; when full, rely on the global limiter instead of allocating indefinitely. General authenticated operations share request bounds from the web contract; monitor mutations additionally cap at 30 per minute. Log repeated denials as sampled summaries, not one durable row per attack request. Admission responses use 429/503 with appropriate retry hints.

Nginx may enforce tighter limits, but application protections remain necessary if proxy configuration is wrong. Apply authentication before expensive history queries or any target operation.

## Outbound destination policy

Monitoring private services is intentional. Define operator-managed permitted private CIDRs/hosts and ports for the home network reached over WireGuard; a logged-in browser cannot widen this policy. Public unicast destinations may be allowed by default. Deny metadata/link-local, multicast, broadcast and unspecified addresses; loopback requires an explicit narrow exception. Normalize IPv4-mapped IPv6 addresses before policy checks.

Validate resolved addresses at connection time, connect only to validated addresses, and preserve the original TLS hostname. Reject mixed allowed/denied DNS results. Redirects are disabled. Apply the policy to HTTP, TCP, ping, retries and fallback addresses. Prevent DNS rebinding from introducing an unchecked second resolution. These controls follow [OWASP SSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html).

TLS certificate validation is mandatory for HTTPS and Telegram. Do not let process environment variables silently route checks through an unexpected proxy. Exact private network permissions are deployment inputs, not a reason to modify the existing WireGuard topology.

WP-02 configuration accepts at most 64 `[[destination_rules]]` entries. Each
contains a CIDR, optional normalized host, up to 64 nonzero ports, and optional
`ping` (default false). At least one port or ping permission is required. IPv4
mapped CIDRs are rejected; mapped answers are normalized before matching.
Private RFC1918/ULA/shared-address destinations require a matching rule; loopback
requires an exact /32 or /128 grant. Link-local/metadata, unspecified, multicast,
global broadcast, documentation and transition/special-use address ranges remain
denied even under a broad grant. Known cloud-internal endpoints
100.100.100.200/168.63.129.16 are hard-denied. AWS's IPv6 metadata endpoint
`fd00:ec2::254` is also hard-denied before ULA classification or any grant,
including broad CIDRs and exact host/address permissions. See
[AWS's metadata documentation](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/instancedata-data-retrieval.html).
IPv4 CIDR network/broadcast addresses
are excluded from matching private grants with prefixes shorter than /31.
Only global unicast 2000::/3 outside the excluded IPv6 special-use ranges is
allowed by default. These conservative exclusions are not a complete future
registry-update promise.

The shared target parser rejects credentials, fragments, unsupported schemes,
control/whitespace characters, ambiguous numeric hosts and repaired authorities.
`Policy::authorize` has a defensive library ceiling of 32 supplied addresses,
rejects any denied member, and returns a capability with normalized numeric addresses and
the original normalized TLS hostname. Literal targets must match every answer.
Monitoring transports retain the stricter [eight-answer limit](../design/monitoring.md#probe-definitions):
WP-03 must reject an entire result containing more than eight DNS answers, without
truncating to eight or selecting an allowed subset. It must resolve once under
bounded admission/deadline, pass every accepted answer to the policy before
family selection, and connect only to capability addresses on
every attempt. No resolver or probe transport is implemented in WP-02; live
rebinding, redirects and verified TLS must additionally be tested at that boundary.

## D-21 all-TCP grant extension for WP-03

The current WP-03 transports resolve once, reject entire sets over eight, validate
all answers before family selection, then attempt only numeric capability
addresses. Auto uses at most two sequential attempts within a shared deadline.
HTTP/TLS do not follow redirects, use environment proxies, retry automatically,
download bodies, store cookies or decompress. Original Host/path/TLS name are
preserved. Optional operator-only `extra_trust_roots` accepts up to 16 DER files
(16 KiB each, 128 KiB total); verification cannot be disabled. Native ping requires
neither raw sockets nor shell commands. Live controlled transport, hostname/TLS,
whole-set/fallback and narrow-group evidence is in [WP-03](../agents/reports/WP-03.md).

Owner-authorized production grants are `192.168.1.0/24` and `10.172.73.1/32`,
all TCP ports and ping in both. Add an explicit `all_tcp_ports` boolean, default
false, to the rule schema. When true it permits ports 1–65535 for HTTP/HTTPS/TCP;
`ports` must be empty/omitted. When false, retain the existing at-most-64 explicit
nonzero ports. Reject conflicting nonempty `ports` plus `all_tcp_ports=true`.
An empty port list by itself grants no TCP access; `ping` remains independent.
At least one of an explicit port, all-TCP permission or ping must be enabled.

WP-03 implements this extension; the historical WP-02 binary does not. Keep
the 64-rule cap and constant-size all-TCP representation. Do not enumerate every
port, change WireGuard/Nginx or auto-enable private ranges. Existing metadata,
network/broadcast, mixed-DNS and exact-loopback checks take precedence. Preserve
public-destination behavior; these grants authorize only the named private ranges.
Test both authorized ranges, ungranted private neighbors, ports 1/65535, port 0
rejection, independent ping permission and hard denials under all-TCP grants.

## Browser and secrets

Use automatic HTML escaping, no rendering target data as HTML, parameterized SQL and a restrictive CSP (`self` assets, no framing, no objects, no unsafe inline script). Add no-sniff, a restrictive referrer policy and `Cache-Control: no-store` for authenticated HTML/API data. All resources are local except server-side Telegram/target connections.

Telegram token lives in a service-readable secret file with restrictive permissions; session and password material never appear in logs or API responses. SQLite contains hashes and operational data, and remains sensitive even without the bot token. Redact outgoing URL query strings, tokens in Telegram paths, cookies, credentials and raw network errors. Backup access is restricted to the operator.

WP-04 reads optional `telegram_token_file` during bounded configuration loading,
with no application state creation. It requires a regular nonsymlink file, at
most 256 bytes, valid token syntax and private Unix permissions. Provision it
outside the repository; Windows ACL restriction remains an operator obligation.
The credential has no Debug/Serialize implementation and its owning buffer is
zeroized. Only its hash fingerprint is persisted to detect changes; neither hash
nor token reaches private/public JSON. Browser settings accept only revisions,
enablement and a signed chat ID. Secret changes cancel/audit old queued jobs.
The official API hostname, HTTPS port and public roots are fixed in code,
independent of target grants and custom probe roots. The whole DNS set must be
public and at most eight answers; pre-POST numeric fallback is capped at two.
No proxy environment, redirects, raw error/description reporting or incoming bot
commands are introduced. All notification routes share existing authentication,
Origin/CSRF, body/deadline and response-size guards.

## Host and proxy requirements

- Only Nginx publishes the dashboard; Rust binds loopback. Trust proxy identity headers only from the configured local Nginx peer, which overwrites incoming forwarded values.

WP-02 accepts optional `trusted_proxy` as one loopback IP. A matching socket peer
must supply exactly one numeric `X-Real-IP` on non-health routes; absent, duplicate
or malformed values are rejected. All other peers use their accepted socket IP;
without the option all identity headers are ignored. `X-Forwarded-For`,
`X-Forwarded-Host` and `X-Forwarded-Proto` never grant authority. Nginx integration
must overwrite `X-Real-IP` and forward the canonical Host. Non-health Host must
exactly match the configured origin's authority, and mutations require its exact
Origin. Absolute-form request targets are rejected. Health uses socket locality
alone and must be excluded from Nginx publishing. A trusted local proxy is an
operator trust boundary, not proof of external-client identity by itself.
- Add limits to the monitor virtual host, not all existing sites. Initial Nginx candidates: five requests/second per IP with burst ten; five login requests/minute with burst five; ten concurrent requests/connections per IP and a measured virtual-host aggregate limit. Verify actual HTTP/2 behavior and all existing shared-host constraints. [Nginx request](https://nginx.org/en/docs/http/ngx_http_limit_req_module.html) and [connection](https://nginx.org/en/docs/http/ngx_http_limit_conn_module.html) limit modules provide these controls.
- Terminate TLS in existing Nginx; verify renewal, HTTP-to-HTTPS redirect and host matching. Enable HSTS only after checking the site's HTTPS behavior; do not blindly add includeSubDomains or preload to the owner's domains.
- Run as a dedicated non-root user. Restrict writes to state/runtime directories. Use no-new-privileges, restrictive umask, filesystem protections and a restart policy compatible with the installed systemd version. Review sandbox controls against DNS, certificate reads and ping before enabling them.
- Prefer Linux ICMP datagram sockets authorized for the service group through `ping_group_range`. Do not broadly enable all groups, give the main service root, or grant raw-socket capabilities without a separately reviewed design. Missing permission is a visible setup fault.
- Preserve current firewall, Nginx sites and WireGuard routing; apply only reviewed integration changes. No automated network scanning or destructive penetration testing.
