# WP-04 validation

Generated results, logs, screenshots, transfer manifests and archives are kept locally and ignored by Git. The committed runners regenerate these artifacts; evidence links below refer to local outputs.

After WP04-R1 correction (2026-10-18): 91 Windows and 93 actual Fedora passing tests,
formatting, warning-denied clippy, release build/version and controlled lifecycle;
Linux interactive account/session regression also passes. The ordinary Fedora
suite skips one existing namespace-only ping-permission case. Retained summaries
are in `results/windows.json` and `results/fedora.json`; see the
[WP-04 report](../../plan/agents/reports/WP-04.md) for exact evidence and limits.

Four library regressions in `storage::notifications::claim_reply_tests` commit
actual SQLite claims and withhold/drop replies past the caller's Tokio deadline.
They verify same-sender recovery/admission/health, repeated reconciliation after
a lost fence reply, expiry/tenth attempts and cancellation/cooldown. The existing
failed-Finish/pinned-WAL and restart-duplicate integration cases remain in the
full suite. Baseline summaries are preserved as `results/*-before-WP04-R1.json`;
earlier raw logs are archived under the ignored evidence directory.

Run `run-windows.ps1` from the repository root with the existing stable alias
and project Cargo cache. If script-file policy blocks invocation, execute the
reviewed contents as a PowerShell scriptblock; do not alter system policy.
Checks use locked/offline dependencies, fmt, warning-denied clippy, full tests,
release build/version and controlled process/SQLite/HTTP lifecycle.

Linux validation is prepared for a fresh isolated checkout at
`/home/gabriele/rust-develop/minimalMonitor/validation/wp04/workspace` using the
documented owner-managed SSH configuration with strict host-key verification.
`prepare-transfer.py` selects application sources/tests/assets/config/scripts
and hashes normalized bytes. It never traverses credentials, excluded folders,
build caches or private configuration. After authorized transfer,
`extract-isolated.py` verifies previous/current hashes before writing sources;
unrelated remote changes stop extraction. Run `bash validation/wp04/run-linux.sh`
there. It reuses the existing project-local WP-03 dependency cache, keeps builds,
temporary state and evidence in the WP-04 checkout and installs no shared tooling.

Tests cover fixed-origin HTTPS, plaintext JSON, disabled previews, TLS root/name
verification, numeric fallback before POST, whole-set DNS rejection, streaming
and length-delimited response limits, total timeout, stalled 429 preservation,
five-second digests, 15 monitors, ordering/resolved incidents, shared-bot cooldown,
permanent errors, restart/duplicates, attempt/expiry bounds, settings revisions,
CSRF, signed chat precision, rate limiting, suppression and terminal cleanup.
Actual SQLite WAL pressure verifies acknowledgement recovery without another
transport call. Production tokens, real channels and third-party traffic are
not used. Live posting rights, GUI, Debian runtime, full resource/soak and
deployment gates remain separate release evidence.
