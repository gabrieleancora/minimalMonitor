# Technical references

Consulted on 2026-10-06. These primary references inform design decisions; resource figures in this plan are original project budgets and must be measured. Library/API defaults and provider limits may change; agents recheck at dependency pinning and release review.

| Source | Use in this plan |
| --- | --- |
| [Axum documentation](https://docs.rs/axum/latest/axum/) | Candidate Rust web framework and middleware integration |
| [reqwest documentation](https://docs.rs/reqwest/latest/reqwest/) | Candidate outbound HTTP client; audit feature flags and actual limits |
| [rusqlite documentation](https://docs.rs/rusqlite/latest/rusqlite/) | Candidate SQLite binding and backup support |
| [SQLite WAL](https://www.sqlite.org/wal.html) | Checkpoint behavior and long-reader growth risk |
| [SQLite PRAGMAs](https://www.sqlite.org/pragma.html) | Cache/page/size/checkpoint configuration to verify during implementation |
| [SQLite backup API](https://www.sqlite.org/backup.html) | Supported live database snapshot mechanism |
| [OWASP password storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) | Argon2id starting parameters |
| [OWASP CSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html) | Session mutation protection |
| [OWASP SSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html) | DNS/redirect/destination policy |
| [Nginx request limits](https://nginx.org/en/docs/http/ngx_http_limit_req_module.html) | Scoped request throttling |
| [Nginx connection limits](https://nginx.org/en/docs/http/ngx_http_limit_conn_module.html) | Scoped concurrent request/connection controls |
| [systemd execution documentation source](https://github.com/systemd/systemd/blob/main/man/systemd.exec.xml) | Host isolation options; match installed version |
| [Rust target support](https://doc.rust-lang.org/rustc/platform-support.html) | Windows/Linux target planning |
| [Linux IP sysctls](https://docs.kernel.org/networking/ip-sysctl.html) | Ping socket group permission |
| [Windows IcmpSendEcho2](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho2) | Native IPv4 ping adapter |
| [Windows Icmp6SendEcho2](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2) | Native IPv6 ping adapter |
| [Telegram Bot API](https://core.telegram.org/bots/api) | Outbound sendMessage and error contract |
| [Telegram bot FAQ](https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this) | Provider rate guidance; conservative local delivery limits |
| [Uptime Kuma repository](https://github.com/louislam/uptime-kuma) | Requested visual/product reference |
| [CISA DDoS guidance](https://www.cisa.gov/sites/default/files/publications/understanding-and-responding-to-ddos-attacks_508c.pdf) | Upstream mitigation and operational response |

No source files, images, executable code or configurations were copied from these references. The graphical interface is specified in prose and a wireframe; implementation remains future agent work.
