# WP-03 validation

Generated results, logs, screenshots, transfer manifests and archives are kept locally and ignored by Git. The committed runners regenerate these artifacts; evidence links below refer to local outputs.

Run `run-windows.ps1` from the repository root using the existing stable toolchain
and project Cargo cache. Where Windows script-file policy refuses invocation,
execute the reviewed script as a PowerShell scriptblock; do not change policy.
The runner performs fmt, warning-denied clippy, locked/offline tests, release
build/version and a controlled process/SQLite/HTTP lifecycle.

Fedora validation uses the owner-documented SSH connection and an **isolated**
checkout at `/home/gabriele/rust-develop/minimalMonitor/validation/wp03/workspace`.
`prepare-transfer.py` packs explicitly selected application/tests/assets/scripts
and records normalized-byte hashes. Transfer the archive, manifest and
`extract-isolated.py` into that checkout, then run extraction and `run-linux.sh`.
Do not overwrite the primary development source. For an update, upload as
`source-next.tar`/`transfer-manifest-next.json`; extraction verifies existing files
against the previous manifest before applying the new selected source.

The Fedora runner keeps dependencies, build products, temporary state and evidence
inside that isolated checkout; stable override and disabled rustup auto-install
preserve shared tooling. Its narrow-ping test verifies a separate user/network
namespace, tests the actual adapter's denied permission, then permits only the
test GID and drops all capabilities. It never changes host-wide ping policy.

Retrieve selected evidence into ignored `evidence/fedora`, run
`collect-results.py`, and retain summaries under `results`. The dependency-review
script checks the exact lockfile against OSV and records license expressions.
Raw logs, transfer manifests and archives are ignored. No credential/private-key
files, prompts, binaries or build caches are transferred from the owner tree.

Runtime scheduling tests use an injected clock and controlled loopback HTTP
targets. They prove cadence/state/coverage and four-probe admission at 15 monitors;
they do not measure baseline p99 delay, API p95, CPU, RAM, retained-history disk
size, a 24-hour soak or production Nginx/WireGuard/Debian compatibility. Telegram
sender/digests/retries and full GUI remain later work packages.
