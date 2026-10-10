"""Finite Linux harness sampler; run from feasibility/wp00, optionally in a scope.

All outputs stay under this prototype. Cgroup readings include this sampler;
process RSS and wait4 peak cover the Rust child separately.
"""
import csv
import json
import os
from pathlib import Path
import subprocess
import time

prototype = Path.cwd().resolve()
project = prototype.parent.parent
binary = prototype / "target/release/minimalmonitor-wp00"
evidence = prototype / "evidence/fedora"
evidence.mkdir(parents=True, exist_ok=True)


def read_int(path):
    try:
        value = path.read_text().strip()
        return int(value) if value != "max" else value
    except (FileNotFoundError, PermissionError):
        return None


def cgroup_path():
    for line in Path("/proc/self/cgroup").read_text().splitlines():
        if line.startswith("0::"):
            return Path("/sys/fs/cgroup") / line[3:].lstrip("/")
    raise RuntimeError("cgroup v2 not available")


group = cgroup_path()
env = os.environ.copy()
env["TMPDIR"] = str(project / ".tmp")
start = time.monotonic()
samples = []
with (evidence / "capped.stdout.txt").open("w") as stdout, (
    evidence / "capped.stderr.txt"
).open("w") as stderr:
    child = subprocess.Popen([str(binary)], cwd=prototype, env=env, stdout=stdout, stderr=stderr)
    while True:
        waited_pid, wait_status, usage = os.wait4(child.pid, os.WNOHANG)
        if waited_pid:
            child.returncode = os.waitstatus_to_exitcode(wait_status)
            break
        try:
            status = Path(f"/proc/{child.pid}/status").read_text()
        except FileNotFoundError:
            continue
        fields = {}
        for line in status.splitlines():
            name, _, value = line.partition(":")
            if name in ("VmRSS", "VmHWM", "VmSwap"):
                fields[name] = int(value.split()[0]) * 1024
        samples.append({
            "elapsed_ms": round((time.monotonic() - start) * 1000, 3),
            "rss_bytes": fields.get("VmRSS", 0),
            "process_peak_rss_bytes": fields.get("VmHWM", 0),
            "process_swap_bytes": fields.get("VmSwap", 0),
            "cgroup_current_bytes": read_int(group / "memory.current"),
            "cgroup_peak_bytes": read_int(group / "memory.peak"),
            "cgroup_swap_bytes": read_int(group / "memory.swap.current"),
        })
        time.sleep(0.01)
    exit_code = child.returncode

# wait4 returns this child's usage only. Aggregate RUSAGE_CHILDREN can include
# pre-existing descendants across exec (for example the preceding Cargo builds).
idle = [s["rss_bytes"] for s in samples if 1000 <= s["elapsed_ms"] <= 2800]
summary = {
    "exit_code": exit_code,
    "samples": len(samples),
    "requested_sample_interval_ms": 10,
    "elapsed_seconds": time.monotonic() - start,
    "idle_rss_min_bytes": min(idle) if idle else None,
    "idle_rss_max_bytes": max(idle) if idle else None,
    "child_getrusage_peak_rss_bytes": usage.ru_maxrss * 1024,
    "child_cpu_user_seconds": usage.ru_utime,
    "child_cpu_system_seconds": usage.ru_stime,
    "observed_child_swap_max_bytes": max(s["process_swap_bytes"] for s in samples),
    "cgroup_path": str(group),
    "cgroup_memory_high": read_int(group / "memory.high"),
    "cgroup_memory_max": read_int(group / "memory.max"),
    "cgroup_swap_max": read_int(group / "memory.swap.max"),
    "cgroup_observed_peak_bytes": max(s["cgroup_peak_bytes"] or 0 for s in samples),
    "cgroup_observed_swap_max_bytes": max(s["cgroup_swap_bytes"] or 0 for s in samples),
    "cgroup_memory_events": (group / "memory.events").read_text(),
    "cgroup_accounting_note": "Includes Python sampler and scope setup; RSS covers Rust child.",
    "executable_bytes": binary.stat().st_size,
    "final_state_bytes": sum(p.stat().st_size for p in (prototype / "target/wp00-state").glob("*") if p.is_file()),
}
with (evidence / "capped-memory.csv").open("w", newline="") as output:
    writer = csv.DictWriter(output, fieldnames=samples[0].keys())
    writer.writeheader()
    writer.writerows(samples)
(evidence / "capped-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
raise SystemExit(exit_code)
