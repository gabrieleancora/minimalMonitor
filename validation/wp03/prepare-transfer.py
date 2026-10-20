"""Explicit source selection, baseline/current hashes, safe repeatable transfer."""
import hashlib
import json
import pathlib
import subprocess
import tarfile
import time

root = pathlib.Path.cwd()
files = [pathlib.Path(p) for p in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "README.md")]
for folder in ("src", "tests", "migrations", "assets", "config"):
    files.extend(p.relative_to(root) for p in (root / folder).rglob("*") if p.is_file())
files.extend(pathlib.Path(p) for p in ("validation/wp02/interactive-linux.py", "validation/wp03/lifecycle.py", "validation/wp03/run-linux.sh", "validation/wp03/narrow-ping.sh"))
manifest = {}
for relative in files:
    data = (root / relative).read_bytes().replace(b"\r\n", b"\n")
    baseline = subprocess.run(["git", "show", f"HEAD:{relative.as_posix()}"], capture_output=True)
    manifest[relative.as_posix()] = {"current": hashlib.sha256(data).hexdigest(), "baseline": hashlib.sha256(baseline.stdout.replace(b"\r\n", b"\n")).hexdigest() if baseline.returncode == 0 else None}
out = root / "validation/wp03"
(out / "transfer-manifest.json").write_text(json.dumps(manifest, indent=2)+"\n", encoding="utf-8")
with tarfile.open(out / "source.tar", "w") as archive:
    import io
    for relative in files:
        data = (root / relative).read_bytes().replace(b"\r\n", b"\n")
        info = tarfile.TarInfo(relative.as_posix())
        info.size = len(data)
        info.mode = 0o644
        info.mtime = time.time() # Cargo tracks source mtimes; never restore epoch zero.
        archive.addfile(info, io.BytesIO(data))
print(f"Prepared {len(files)} selected files; no credentials, caches or prompts.")
