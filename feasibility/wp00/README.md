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
The Linux adapter is source-only in this report's environment, not validated.

Boundaries are `storage`, `hashing`, `transport`, and `ping`. Fixtures are local
controlled servers, not hardened application listeners. No authentication,
scheduler, history, outbox or migrations are implemented. The HTTP fixture serves
static synthetic content only. The test `characterize_reqwest_http1_header_limit_gap`
passes when it reproduces a **failed security constraint** (33 KiB accepted). It
must not be read as header-limit acceptance evidence. Production requires a
pre-allocation parser limit, connection/header/body admission controls and shared
outbound policy; see the report. Do not promote this crate directly to production.
