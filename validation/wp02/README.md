# WP-02 security validation

Run from the repository root using the existing stable installation and a
project-local Cargo cache. Set `RUSTUP_AUTO_INSTALL=0` to prevent implicit shared
toolchain installation. The exact selected compiler is recorded by each runner.

Windows: invoke `run-windows.ps1` in an allowed PowerShell script session, or
execute its contents with `& ([scriptblock]::Create((Get-Content -LiteralPath
validation/wp02/run-windows.ps1 -Raw)))`. This does not change execution policy.
Fedora: follow the [approved access/transfer boundary](../../plan/operations/linux-development.md)
and run `bash validation/wp02/run-linux.sh` from the approved project root.

Both runners execute fmt, warning-denied clippy, all locked tests, release build,
version and [release lifecycle](lifecycle.py). The release lifecycle retains
foundation ownership/crash/restart/schema checks and adds protected routes,
canonical Host and noninteractive account-command refusal. Fedora additionally
runs [real interactive commands](interactive-linux.py) using a private PTY,
without recording passwords or cookies. It checks bootstrap/reset, duplicate
bootstrap refusal, no echo, login/logout, persisted reset revocation and SIGTERM.
Windows uses forced process termination and does not claim terminal no-echo
or graceful console/service testing.

For the short release security memory exercise on Fedora:

```bash
systemd-run --user --scope --quiet --unit=minimalmonitor-wp02-security-measure \
  -p MemoryHigh=96M -p MemoryMax=128M -p MemorySwapMax=0 \
  python3 validation/wp02/measure-linux.py target/release/minimalmonitor
```

Use an unused unit name. The [sampler](measure-linux.py) observes controlled Rust
child RSS/HWM and scope current/kernel peak including Python/setup, verifies the
cap/swap settings, and checks OOM events. This is a short account exercise, not
scheduler load, incremental Nginx measurement, sustained CPU/API latency or the
24-hour release soak.

`python validation/wp02/dependency_review.py` reviews the current lockfile via
OSV and Cargo license metadata; use the same toolchain/cache environment. Raw
logs are ignored under `evidence/`. Retained summaries:
[Windows](results/windows.json), [Fedora](results/fedora.json),
[capped exercise](results/capped.json), [advisories](results/dependencies.json)
and [licenses](results/licenses.json). See the [handoff report](../../plan/agents/reports/WP-02.md)
for exact scope and unrun release gates. Fixture state stays under `target/`;
no external target is contacted by functional tests.

The metadata-review follow-up adds unconditional `fd00:ec2::254` denial and a
regression covering literal/DNS/mixed/broad/exact/host grants. Current runners
pass 33 Windows/34 Fedora tests; pre-review summaries are retained separately.
The capped measurement predates this fix and was not rerun. The transport DNS
limit remains eight, with no truncation, while the policy-helper ceiling is 32.
