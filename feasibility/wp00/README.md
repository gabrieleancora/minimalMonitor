# WP-00 feasibility prototype

Finite local experiment only; not the application or a deployment artifact.
Run commands from this directory with stable Rust and the platform C toolchain
(bundled SQLite and ring compile C). The tested Windows toolchain is recorded in
[the report](../../plan/agents/reports/WP-00.md); Cargo.lock fixes dependency resolution.

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

On Windows run `./measure-windows.ps1` after the release build. It launches the
finite harness with a hidden window and records output, 10 ms requested memory
samples, summary, and an 8 KiB synthetic database under ignored `target/`.
Actual sampling frequency depends on Windows scheduling. Run `cargo run
--release --locked` to execute without the memory sampler. The harness uses two
Tokio workers, a 128-command SQLite thread, up to four local probe tasks, and one
admitted Argon2id operation. After the six seconds of observation sleeps it exits.

Only fixed loopback test targets are allowed. No user targets, credentials,
production routes, Nginx/WireGuard changes or Telegram sends exist. The generated
TLS key and self-signed certificate are ephemeral. Hash input/salt are synthetic
test material, not an account or production password-storage implementation.

`python3 linux-environment.py` is a read-only Linux diagnostic for tool availability
and ICMP datagram socket permissions. Run the Rust checks on an equipped Linux
development machine as a non-root user whose explicitly assigned service group
has narrowly authorized ping datagram permission for both IPv4 and IPv6. Do not
enable all groups, add raw-socket capabilities, or run the app as root to make
tests pass. Missing permission intentionally fails the real echo test/harness.
The Linux adapter was compiled/tested on the owner-provided Fedora machine on
2026-10-09, including non-root v4/v6 loopback echo. Its broad existing group policy
is development-only evidence, not the narrow production policy. See [Linux
development access](../../plan/operations/linux-development.md).

On the remote host, run `bash feasibility/wp00/run-linux.sh` from the approved
project root. It keeps Cargo downloads, build/cache/temp/state and evidence under
that root. For the short capped measurement run `measure-linux.py` from this
prototype directory in the temporary user scope documented in the access guide.
The sampler records per-child `wait4` peak RSS, CSV samples, and cgroup limits,
peak/swap/events; cgroup accounting includes the sampler. These scripts overwrite
their named logs, so archive previous evidence before another run.

Boundaries are `storage`, `hashing`, `probe_http`, `transport` fixtures, and `ping`. Fixtures are local
controlled servers, not hardened application listeners. No authentication,
scheduler, history, outbox or migrations are implemented. The HTTP fixture serves
static synthetic content only. The Hyper/rustls probe configures a 32 KiB wire
header limit, 32 KiB connection read-buffer strategy and 100-header count limit.
It uses one connection, polls the driver within the probe future, drops bodies
unread and imposes one total deadline across connect/TLS/headers. It does not
follow redirects, retry, inherit proxies, persist cookies or decompress bodies.
Windows/Fedora tests accept the exact byte boundary and reject single/cumulative/
fragmented/unterminated/excess-count responses, including HTTP/HTTPS tests.

The dev-only test `characterize_reqwest_http1_header_limit_gap` still reproduces
the old client's gap for comparison; reqwest is absent from the prototype's release
dependency tree. It remains a proposed option for the later Telegram sender.
Production still requires shared outbound policy, connection/body/request admission
and authentication; do not promote this crate directly to production.

`narrow-ping.sh`, run from the approved remote root, additionally creates a
disposable user/network namespace, brings up only its loopback, sets only its
namespace-local ping range to the current single GID, and drops every capability
before the Rust IPv4/IPv6 test. It fails if the network namespace has not changed
or the test UID/GID is root. No host sysctl is written. See the report for the
observed denied-default, one-GID success and unchanged host-policy evidence.
