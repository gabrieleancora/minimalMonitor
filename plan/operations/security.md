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

- Bootstrap the only admin through a local interactive command; no default credentials, unauthenticated setup route, or open signup. Prompt securely rather than putting passwords in process arguments or logs.
- Local bootstrap/reset acquires exclusive process ownership with the service stopped. A reset revokes sessions before restart; it does not create a second concurrent database/scheduler owner.
- Use Argon2id with a starting minimum of 19 MiB, two iterations, one lane, unique random salt and stored parameters. Benchmark on the host and retain only one concurrent hash operation; reject excess work rather than queue indefinitely. This baseline comes from [OWASP password-storage guidance](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).
- Propose passwords of 15–128 characters, no mandatory composition rules, no silent truncation. Cap encoded credential input at 1024 bytes, username at 64 characters. Allow paste and password managers.
- Login errors do not reveal whether the username exists. Unknown users take equivalent bounded hash work. Apply rate limits before hashing.
- Use cryptographically random 256-bit opaque session tokens; store only token hashes. Cookie: `__Host-` prefix, Secure, HttpOnly, SameSite=Lax, Path=/, no Domain. No authentication tokens in local storage.
- Expire after two hours idle or 24 hours absolute, whichever first; cap five active sessions, revoking oldest when exceeded. Rotate token on login. Logout, local reset and password change revoke affected sessions immediately. Password change requires the current password and invalidates all sessions.
- Enforce authorization centrally for every private route. Use a production canonical origin/host allowlist; do not trust arbitrary Host or forwarded headers.
- Protect all mutations, including login/logout, with CSRF tokens and origin checks. Use a signed, short-lived pre-auth token/cookie flow without an unbounded anonymous session table. SameSite is an additional defense, not the sole control. [OWASP CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).

## Application limits

Initial login limits: five attempts per minute per client IP with burst five, and six hash starts per minute globally with burst two. No permanent username lockout that an attacker can trigger. An attacker can still temporarily consume public login capacity; expose an operational metric and preserve local recovery.

Rate-limit maps have a 2048-entry cap and expiry; when full, rely on the global limiter instead of allocating indefinitely. General authenticated operations share request bounds from the web contract; monitor mutations additionally cap at 30 per minute. Log repeated denials as sampled summaries, not one durable row per attack request. Admission responses use 429/503 with appropriate retry hints.

Nginx may enforce tighter limits, but application protections remain necessary if proxy configuration is wrong. Apply authentication before expensive history queries or any target operation.

## Outbound destination policy

Monitoring private services is intentional. Define operator-managed permitted private CIDRs/hosts and ports for the home network reached over WireGuard; a logged-in browser cannot widen this policy. Public unicast destinations may be allowed by default. Deny metadata/link-local, multicast, broadcast and unspecified addresses; loopback requires an explicit narrow exception. Normalize IPv4-mapped IPv6 addresses before policy checks.

Validate resolved addresses at connection time, connect only to validated addresses, and preserve the original TLS hostname. Reject mixed allowed/denied DNS results. Redirects are disabled. Apply the policy to HTTP, TCP, ping, retries and fallback addresses. Prevent DNS rebinding from introducing an unchecked second resolution. These controls follow [OWASP SSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html).

TLS certificate validation is mandatory for HTTPS and Telegram. Do not let process environment variables silently route checks through an unexpected proxy. Exact private network permissions are deployment inputs, not a reason to modify the existing WireGuard topology.

## Browser and secrets

Use automatic HTML escaping, no rendering target data as HTML, parameterized SQL and a restrictive CSP (`self` assets, no framing, no objects, no unsafe inline script). Add no-sniff, a restrictive referrer policy and `Cache-Control: no-store` for authenticated HTML/API data. All resources are local except server-side Telegram/target connections.

Telegram token lives in a service-readable secret file with restrictive permissions; session and password material never appear in logs or API responses. SQLite contains hashes and operational data, and remains sensitive even without the bot token. Redact outgoing URL query strings, tokens in Telegram paths, cookies, credentials and raw network errors. Backup access is restricted to the operator.

## Host and proxy requirements

- Only Nginx publishes the dashboard; Rust binds loopback. Trust proxy identity headers only from the configured local Nginx peer, which overwrites incoming forwarded values.
- Add limits to the monitor virtual host, not all existing sites. Initial Nginx candidates: five requests/second per IP with burst ten; five login requests/minute with burst five; ten concurrent requests/connections per IP and a measured virtual-host aggregate limit. Verify actual HTTP/2 behavior and all existing shared-host constraints. [Nginx request](https://nginx.org/en/docs/http/ngx_http_limit_req_module.html) and [connection](https://nginx.org/en/docs/http/ngx_http_limit_conn_module.html) limit modules provide these controls.
- Terminate TLS in existing Nginx; verify renewal, HTTP-to-HTTPS redirect and host matching. Enable HSTS only after checking the site's HTTPS behavior; do not blindly add includeSubDomains or preload to the owner's domains.
- Run as a dedicated non-root user. Restrict writes to state/runtime directories. Use no-new-privileges, restrictive umask, filesystem protections and a restart policy compatible with the installed systemd version. Review sandbox controls against DNS, certificate reads and ping before enabling them.
- Prefer Linux ICMP datagram sockets authorized for the service group through `ping_group_range`. Do not broadly enable all groups, give the main service root, or grant raw-socket capabilities without a separately reviewed design. Missing permission is a visible setup fault.
- Preserve current firewall, Nginx sites and WireGuard routing; apply only reviewed integration changes. No automated network scanning or destructive penetration testing.
