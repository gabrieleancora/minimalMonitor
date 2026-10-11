# Available Linux development machine

Updated: 2026-10-10. The owner supplied this remote development machine for
minimalMonitor. It is separate from the resource-constrained monitor host.
Use it within the owner's assigned work package; availability does not authorize
starting another package, deploying production software or dispatching agents.

## Access and project boundary

- SSH alias: `remote-dev`, configured in the local repository's `.ssh/config`
  (not the Windows profile's `.ssh/config`). Use the configured non-root account.
- Approved remote project root: **`/home/gabriele/rust-develop/minimalMonitor`**.
  The folder exists; WP-00 is under `feasibility/wp00` inside it.
- Keep all remote project source, dependency downloads, build products, temporary
  files, state, archives and evidence in that root. Inspect existing files before
  transfer, preserve unrelated work, and avoid blindly replacing a remote tree.
- Read the preinstalled toolchain through its existing PATH. Do not install or
  update shared tooling, alter host sysctls/services, or work in other project
  folders without an assignment covering those changes.

SSH credentials and host records are owner-managed local files under `.ssh/`,
which is ignored by Git. Never copy private keys to the server, source, logs or
reports. The ED25519 fingerprint below was explicitly verified by the owner:

```text
SHA256:lrx8l1dM1HAEuz7eswnhRkDH2tRHQ/UwOZSXBryL1zk
```

Keep strict host-key checking enabled and use `.ssh/known_hosts`. Stop if the
presented key changes. On Windows, the current `IdentityFile` entry references
`gab2026.lnk`; OpenSSH does not resolve Windows shortcuts. Resolve its target
locally and pass the path with `-i`, without reading/printing key contents:

```powershell
# Run from the local minimalMonitor repository root.
$shortcutPath = Join-Path (Get-Location) '.ssh/gab2026.lnk'
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
$keyPath = $shortcut.TargetPath
ssh -F .ssh/config -i $keyPath `
    -o UserKnownHostsFile=.ssh/known_hosts `
    -o StrictHostKeyChecking=yes -o BatchMode=yes remote-dev `
    'cd /home/gabriele/rust-develop/minimalMonitor && pwd'
```

The private key is readable by the owner account; the Codex sandbox returned
`Permission denied` while opening it. Authorized SSH/SCP calls succeeded outside
that sandbox. Request the normal sandbox escalation for those calls if required;
do not loosen private-key permissions. Recheck current config if the owner updates it.
For transfers use SCP with the same `-F`, `-i`, known-host and strict-check options,
and explicit destinations underneath the approved remote root. Do not transfer
`.ssh/`, `prompts/`, Windows binaries or build caches.

## Observed toolchain and host (2026-10-09)

| Item | Observed value |
| --- | --- |
| Distribution / kernel | Fedora Linux 44 Workstation; 7.1.4-200.fc44.x86_64 |
| Architecture | x86_64 / Rust x86_64-unknown-linux-gnu |
| Rust / Cargo | rustc 1.99.0 (b940084d7); cargo 1.99.0 (5f94df478) |
| Other tools | rustfmt, clippy, GCC 16.2.1, Python 3, GNU time, systemd-run |
| libc / storage | glibc 2.43; project root on local Btrfs |
| CPU / RAM | Ryzen 5 7600X host, six virtual logical CPUs; 8,384,749,568 bytes RAM |
| Test identity | UID/GID 1000; non-root |
| Ping policy | Existing `ping_group_range=0 2147483647`; v4/v6 datagram echo succeeds |

The existing ping range allows every group. It was not changed for WP-00 and
must not be copied to production; production still requires the narrow reviewed
service-group policy. The host has swap enabled; the recorded capped experiment
used `MemorySwapMax=0`. Fedora/glibc 2.43 is a development/test baseline, not the
selected production minimum. Production is now confirmed as Debian 12/glibc 2.36
(D-14, 2026-10-11); building natively here does not prove that compatibility.
Build and execute against the production baseline before delivery.

On 2026-10-10, WP-00 also verified IPv4 and IPv6 datagram echo in a temporary
user/network namespace with `ping_group_range=1000 1000`, UID/GID 1000, all
capabilities dropped and `NoNewPrivs=1`. Both families first failed under the
namespace's default range, then passed with only the test group permitted.
The host-wide range remained unchanged. This proves the narrow-group mechanism
on this development kernel; production still needs its own reviewed service
group and platform verification.

## Repeating the WP-00 checks

After copying only the current WP-00 source/scripts and lockfile, run from the
approved remote root:

```bash
bash feasibility/wp00/run-linux.sh
```

To repeat the isolated narrow-group ping experiment from the approved project
root, run `bash feasibility/wp00/narrow-ping.sh`. The script verifies that it is
in a separate network namespace before changing that namespace's ping range,
brings up its loopback interface, and drops capabilities before running the
IPv4/IPv6 test. Do not replace this with a host-wide sysctl change. User/network
namespaces and the existing `unshare`, `ip` and `setpriv` tools must be available.

The script sets `CARGO_HOME` to `<project>/.cargo`, `CARGO_TARGET_DIR` to
`<project>/feasibility/wp00/target`, and `TMPDIR` to `<project>/.tmp`. It runs fmt,
clippy, tests, release build and GNU-time measurement, collecting individual exit
codes and logs under `feasibility/wp00/evidence/fedora`. It reads the preinstalled
Rust toolchain and does not change system configuration. Review/archive old
evidence before rerunning, because the script overwrites its named logs.

For the short capped release experiment, run from the remote prototype directory:

```bash
cd /home/gabriele/rust-develop/minimalMonitor/feasibility/wp00
systemd-run --user --scope --quiet --unit=minimalmonitor-wp00-measure \
  -p MemoryHigh=96M -p MemoryMax=128M -p MemorySwapMax=0 \
  python3 measure-linux.py
```

This creates a temporary user scope, without a persistent service installation.
The sampler uses per-child `wait4` resource accounting and records Rust RSS
separately from cgroup memory (which includes the Python sampler and scope setup).
Archive previous capped logs before repeating, and use an unused unit name if
another experiment is active. Retrieve only evidence into the local repository;
build outputs and Cargo caches remain development-only. These commands test the
small prototype, not the 24-hour application qualification or production Nginx/
WireGuard integration. See [WP-00 results](../agents/reports/WP-00.md).
