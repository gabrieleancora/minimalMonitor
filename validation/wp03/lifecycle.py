"""Controlled WP-03 process/SQLite/HTTP checks, Windows and Linux.

All files are under target/wp03-lifecycle-<unique>. No external targets or secrets.
Linux additionally verifies SIGTERM shutdown and private state permissions.
"""
import hashlib
import http.client
import json
import os
import pathlib
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time


def main():
    root = pathlib.Path.cwd()
    binary = pathlib.Path(sys.argv[1]).resolve()
    (root / "target").mkdir(exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="wp03-lifecycle-", dir=root / "target"))
    config = work / "config.toml"
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    config.write_text(f'mode="development"\nlisten="127.0.0.1:{port}"\nstate_dir="state"\npublic_origin="http://localhost:{port}"\n', encoding="utf-8")
    children = []

    def launch():
        # The service emits no per-request logs; PIPE cannot grow with traffic.
        child = subprocess.Popen([str(binary), "serve", str(config)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        children.append(child)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if child.poll() is not None:
                raise RuntimeError(f"early exit: {child.communicate()}")
            try:
                status, body = request("/health/ready")
                if status == 200:
                    assert body == {"status": "ready"}
                    return child
            except (OSError, http.client.HTTPException):
                pass
            time.sleep(0.05)
        raise RuntimeError("readiness deadline exceeded")

    def request(path, method="GET", headers=None):
        client = http.client.HTTPConnection("127.0.0.1", port, timeout=6)
        try:
            client.request(method, path, headers={"Host": f"localhost:{port}", **(headers or {})})
            response = client.getresponse()
            body = response.read(256 * 1024 + 1)
            assert len(body) <= 256 * 1024
            assert response.getheader("Cache-Control") == "no-store"
            assert response.getheader("X-Content-Type-Options") == "nosniff"
            return response.status, json.loads(body) if body.startswith(b'{') else body.decode()
        finally:
            client.close()

    started = time.monotonic()
    try:
        assert subprocess.run([str(binary), "check-config", str(config)], timeout=5).returncode == 0
        assert not (work / "state").exists(), "validation must not create state"
        first = launch()
        status, body = request("/health/live")
        assert (status, body) == (200, {"status": "live"})
        for route in ["/", "/settings"]:
            assert request(route, headers={"X-Forwarded-For": "203.0.113.1"})[0] == 303
        assert request("/api/v1/summary", headers={"X-Forwarded-For": "127.0.0.1"})[0] == 401
        assert request("/login")[0] == 200
        assert request("/login",headers={"Host":"evil.invalid","X-Forwarded-Host":f"localhost:{port}"})[0] == 403
        assert request("/auth/login","POST",headers={"Origin":f"http://localhost:{port}","Content-Type":"application/json"})[0] == 403
        for command in ["admin-bootstrap","admin-reset"]:
            refused=subprocess.run([str(binary),command,str(config)],input=b"never use piped credentials",capture_output=True,timeout=5)
            assert refused.returncode==1
            assert refused.stderr.strip()==b"minimalmonitor: state_already_owned"
        assert request("/health/live", "POST")[0] == 405
        assert request("/health/live", headers={"Content-Length": "16385"})[0] == 413
        second = subprocess.run([str(binary), "serve", str(config)], capture_output=True, timeout=5)
        assert second.returncode == 1
        assert second.stderr.strip() == b"minimalmonitor: state_already_owned"

        # Simulate an abrupt host/process outage: OS must release the persistent
        # lock and SQLite must reopen the WAL without reapplying migration 1.
        first.kill()
        first.communicate(timeout=5)
        refused=subprocess.run([str(binary),"admin-bootstrap",str(config)],input=b"never use piped credentials",capture_output=True,timeout=5)
        assert refused.returncode==1 and refused.stderr.strip()==b"minimalmonitor: invalid_configuration"
        dbpath = work / "state/monitor.db"
        connection = sqlite3.connect(dbpath)
        assert connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert connection.execute("SELECT count(*) FROM schema_versions").fetchone()[0] == 3
        assert connection.execute("SELECT count(*) FROM admin").fetchone()[0] == 0
        connection.execute("UPDATE settings SET revision=7 WHERE id=1")
        connection.commit()
        connection.close()
        restarted = launch()
        if os.name != "nt":
            os.kill(restarted.pid, signal.SIGTERM)
            stdout, stderr = restarted.communicate(timeout=10)
            assert restarted.returncode == 0 and not stdout and not stderr
            assert (work / "state").stat().st_mode & 0o077 == 0
            assert dbpath.stat().st_mode & 0o077 == 0
        else:
            restarted.kill()
            restarted.communicate(timeout=5)
        connection = sqlite3.connect(dbpath)
        assert connection.execute("SELECT revision FROM settings").fetchone()[0] == 7
        connection.execute("PRAGMA user_version=999")
        connection.commit()
        connection.close()
        incompatible = subprocess.run([str(binary), "serve", str(config)], capture_output=True, timeout=5)
        assert incompatible.returncode == 1
        assert incompatible.stderr.strip() == b"minimalmonitor: incompatible_schema"
        with socket.socket() as sock:
            assert sock.connect_ex(("127.0.0.1", port)) != 0, "failed startup must not bind listener"
        summary = {"platform": sys.platform, "passed": True, "seconds": round(time.monotonic() - started, 3), "binary_bytes": binary.stat().st_size, "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(), "state_bytes": sum(p.stat().st_size for p in (work / "state").iterdir() if p.is_file()), "linux_sigterm": os.name != "nt", "windows_forced_exit": os.name == "nt"}
        print(json.dumps(summary, indent=2))
        return 0
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
                child.communicate(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
