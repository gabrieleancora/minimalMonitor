"""Extract selected source ONLY into the fresh, approved validation checkout."""
import hashlib
import json
import pathlib
import tarfile

root = pathlib.Path.cwd().resolve()
assert str(root) == "/home/gabriele/rust-develop/minimalMonitor/validation/wp03/workspace"
old = json.loads((root / "transfer-manifest.json").read_text())
manifest_path=root / "transfer-manifest-next.json"
manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else old
archive_path=root / ("source-next.tar" if manifest_path.exists() else "source.tar")
with tarfile.open(archive_path) as archive:
    assert {m.name for m in archive} == set(manifest)
    for member in archive:
        path = root / member.name
        assert path.resolve().is_relative_to(root)
        if path.exists():
            assert hashlib.sha256(path.read_bytes()).hexdigest() in (manifest[member.name]["current"],old.get(member.name,{}).get("current")), f"Existing changes: {member.name}"
    archive.extractall(root, filter="data")
for relative, expected in manifest.items():
    assert hashlib.sha256((root / relative).read_bytes()).hexdigest() == expected["current"]
if manifest_path.exists():
    (root / "transfer-manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
    manifest_path.unlink()
print(f"Verified {len(manifest)} files in isolated validation checkout.")
