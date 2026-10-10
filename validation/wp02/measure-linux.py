"""Short controlled release account exercise, not sustained resource qualification.
Run in a temporary 128 MiB user scope with swap disabled. Never print secrets.
"""
import json
import pathlib
import subprocess
import sys
import time

binary = pathlib.Path(sys.argv[1]).resolve()
line = next(line for line in pathlib.Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))
cgroup = pathlib.Path('/sys/fs/cgroup') / line.split('::', 1)[1].lstrip('/')
assert (cgroup / 'memory.max').read_text().strip() == str(128 * 1024 * 1024)
assert (cgroup / 'memory.swap.max').read_text().strip() == '0'
child = subprocess.Popen([sys.executable, 'validation/wp02/interactive-linux.py', str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
peak_rss = peak_current = samples = 0
started = time.monotonic()
try:
    while child.poll() is None:
        queue = [child.pid]
        seen = set()
        while queue and len(seen) < 16:
            pid = queue.pop()
            if pid in seen:
                continue
            seen.add(pid)
            try:
                proc = pathlib.Path('/proc') / str(pid)
                queue.extend(int(p) for p in (proc / 'task' / str(pid) / 'children').read_text().split())
                if (proc / 'comm').read_text().strip() == binary.name:
                    status = (proc / 'status').read_text()
                    for field in status.splitlines():
                        if field.startswith(('VmRSS:', 'VmHWM:')):
                            peak_rss = max(peak_rss, int(field.split()[1]) * 1024)
            except (FileNotFoundError, ProcessLookupError):
                pass
        peak_current = max(peak_current, int((cgroup / 'memory.current').read_text()))
        samples += 1
        assert time.monotonic() - started < 60, 'measurement deadline'
        time.sleep(0.005)
    out, err = child.communicate(timeout=5)
    assert child.returncode == 0 and not err
    exercise = json.loads(out)
    assert exercise['status'] == 'pass'
    events = {key: int(value) for key, value in (line.split() for line in (cgroup / 'memory.events').read_text().splitlines())}
    assert events.get('oom', 0) == 0 and events.get('oom_kill', 0) == 0
    assert int((cgroup / 'memory.swap.current').read_text()) == 0
    print(json.dumps({'scenario': 'short release bootstrap/login/logout/reset', 'seconds': round(time.monotonic() - started, 3), 'samples': samples, 'rust_sampled_peak_rss_bytes': peak_rss, 'scope_sampled_peak_current_bytes': peak_current, 'scope_kernel_peak_bytes': int((cgroup / 'memory.peak').read_text()), 'scope_swap_bytes': 0, 'scope_events': events, 'scope_includes_python_and_setup': True, 'exercise': exercise}, indent=2))
finally:
    if child.poll() is None:
        child.kill()
        child.communicate(timeout=5)
