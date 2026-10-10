"""Exercise the real stopped-service account commands using a private Linux PTY.
No terminal output, passwords, cookies or token values are emitted or retained.
"""
import http.client
import json
import os
import pathlib
import pty
import select
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time

binary = pathlib.Path(sys.argv[1]).resolve()
work = pathlib.Path(tempfile.mkdtemp(prefix="wp02-interactive-", dir="target"))
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
config = work / "config.toml"
config.write_text(f'mode="development"\nlisten="127.0.0.1:{port}"\nstate_dir="state"\npublic_origin="http://localhost:{port}"\n')
password = "controlled disposable passphrase"

def account(command, expected=0):
    pid, master = pty.fork()
    if pid == 0:
        os.execl(str(binary), str(binary), command, str(config))
    try:
        prompts = [b"Administrator username: ", b"New password: ", b"Confirm password: "]
        answers = ["fixture", password, password]
        pending = b""
        deadline = time.monotonic() + 10
        for prompt, answer in zip(prompts, answers):
            while prompt not in pending:
                assert time.monotonic() < deadline, "interactive command deadline"
                if select.select([master], [], [], 0.1)[0]:
                    pending += os.read(master, 4096)
                    assert len(pending) < 16384
            pending = pending.split(prompt, 1)[1]
            os.write(master, (answer + "\n").encode())
        while True:
            ended, status = os.waitpid(pid, os.WNOHANG)
            if ended:
                assert os.waitstatus_to_exitcode(status) == expected
                break
            assert time.monotonic() < deadline
            if select.select([master], [], [], 0.1)[0]:
                try:
                    pending += os.read(master, 4096)
                except OSError:
                    pass
        assert password.encode() not in pending, "terminal must not echo password"
    finally:
        os.close(master)
        try:
            os.kill(pid, 9)
        except ProcessLookupError:
            pass

def launch():
    child = subprocess.Popen([str(binary), "serve", str(config)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    for _ in range(100):
        try:
            if request("/health/ready")[0] == 200:
                return child
        except OSError:
            pass
        assert child.poll() is None
        time.sleep(0.05)
    raise RuntimeError("startup deadline")

def stop(child):
    child.terminate()
    out, err = child.communicate(timeout=10)
    assert child.returncode == 0 and not out and not err

def login():
    status, headers, _ = request("/login")
    assert status == 200
    cookie = headers["set-cookie"].split(";", 1)[0]
    csrf = cookie.split("=", 1)[1]
    status, headers, body = request("/auth/login", "POST", {"username": "fixture", "password": password}, {"Cookie": cookie, "X-CSRF-Token": csrf})
    assert status == 200
    # There are two Set-Cookie fields; dict retains the preauth deletion. Get the
    # new session by querying only its SHA-256 would defeat the opacity test, so
    # retain all response cookie fields explicitly in the HTTP helper instead.
    return headers["session-cookie"], json.loads(body)["csrf_token"]

# Preserve repeated Set-Cookie fields without ever printing them.
def request(path, method="GET", data=None, headers=None):
    client = http.client.HTTPConnection("127.0.0.1", port, timeout=6)
    try:
        base = {"Host": f"localhost:{port}", "Origin": f"http://localhost:{port}", "Content-Type": "application/json"}
        client.request(method, path, json.dumps(data) if data is not None else None, {**base, **(headers or {})})
        response = client.getresponse()
        result = dict(response.getheaders())
        for name, value in response.getheaders():
            if name.lower() == "set-cookie" and value.startswith("mm-dev-session="):
                result["session-cookie"] = value.split(";", 1)[0]
        body = response.read(65537)
        assert len(body) <= 65536
        return response.status, result, body
    finally:
        client.close()

account("admin-bootstrap")
account("admin-bootstrap", expected=1)
child = launch()
try:
    cookie, csrf = login()
    assert request("/api/v1/account/session", headers={"Cookie": cookie})[0] == 200
    assert request("/auth/logout", "POST", headers={"Cookie": cookie})[0] == 403
    assert request("/auth/logout", "POST", headers={"Cookie": cookie, "X-CSRF-Token": csrf})[0] == 204
    assert request("/api/v1/account/session", headers={"Cookie": cookie})[0] == 401
    cookie, csrf = login()
finally:
    stop(child)
account("admin-reset")
db = sqlite3.connect(work / "state/monitor.db")
assert db.execute("SELECT count(*) FROM sessions").fetchone()[0] == 0
assert db.execute("SELECT credential_generation FROM admin").fetchone()[0] == 2
assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
db.close()
child = launch()
try:
    assert request("/api/v1/account/session", headers={"Cookie": cookie})[0] == 401
    login()
finally:
    stop(child)
print(json.dumps({"status": "pass", "interactive_bootstrap_reset": True, "terminal_no_echo": True, "http_login_logout_reset_revocation": True, "linux_sigterm": True}))
