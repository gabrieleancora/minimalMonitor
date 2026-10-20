"""Pack selected, nonsecret sources; never traverse owner/private directories."""
import hashlib
import io
import json
import pathlib
import tarfile
import time

root = pathlib.Path.cwd()
files = [pathlib.Path(p) for p in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "README.md")]
for folder in ("src", "tests", "migrations", "assets", "config"):
    files.extend(p.relative_to(root) for p in (root / folder).rglob("*") if p.is_file())
files.extend(pathlib.Path(p) for p in ("validation/wp02/interactive-linux.py", "validation/wp04/lifecycle.py", "validation/wp04/run-linux.sh"))
manifest = {}
out = root / "validation/wp04"
with tarfile.open(out / "source.tar", "w") as archive:
    for relative in files:
        data = (root / relative).read_bytes().replace(b"\r\n", b"\n")
        manifest[relative.as_posix()] = hashlib.sha256(data).hexdigest()
        info = tarfile.TarInfo(relative.as_posix())
        info.size, info.mode, info.mtime = len(data), 0o644, time.time()
        archive.addfile(info, io.BytesIO(data))
(out / "transfer-manifest.json").write_text(json.dumps(manifest, indent=2)+"\n", encoding="utf-8")
print(f"Prepared {len(files)} selected files.")
