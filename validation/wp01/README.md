# WP-01 validation

From the repository root, run `./validation/wp01/run-windows.ps1` on Windows or
`bash validation/wp01/run-linux.sh` on the documented Fedora development machine.
Both record compiler versions, required checks, release version and process
lifecycle evidence. The Linux runner requires the approved project root and keeps
Cargo/temp/build/evidence paths there; it does not install shared tooling.

`lifecycle.py` exercises only loopback: configuration validation without state
creation, safe local health, missing feature routes, wrong method/oversized body,
exclusive process ownership, forced-stop/restart integrity, preserved settings
and refusal of a newer schema before listening. Linux additionally tests SIGTERM
and private state permissions. Windows termination is forced; it is not proof of
graceful Ctrl+C or a Windows service-manager adapter.

`dependency_review.py` checks exact locked registry versions against
[OSV's batch API](https://google.github.io/osv.dev/post-v1-querybatch/) and records
Cargo license metadata. This does not scan application logic or prove that every
native-library CVE is represented. The bundled SQLite runtime version is recorded
separately and reviewed against [SQLite's vulnerability notes](https://www.sqlite.org/cves.html).

Retained package evidence:

- [Windows summary](results/windows.json): 19 tests and all six runner checks passed.
- [Fedora summary](results/fedora.json): 20 tests and all six runner checks passed.
- [Dependency summary](results/dependencies.json): 99 registry versions, no OSV findings at the recorded time.
- [License inventory](results/licenses.json): exact graph, including platform/build/test packages.

The [WAL integration test](../../tests/wal_recovery.rs) demonstrates pressure,
blocked admission with a pinned reader, and recovery on the same worker/client
after reader release. It verifies snapshot preservation, committed data and
integrity; removing the recovery block fails its resumed-admission assertion.

Raw `evidence/` logs and source-transfer archives are ignored development files.
Summary evidence is tracked. The report is [plan/agents/reports/WP-01.md](../../plan/agents/reports/WP-01.md).
No resource soak, production baseline, live target, proxy, Telegram or GUI gate is
claimed by these checks.
