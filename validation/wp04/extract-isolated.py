"""Verify previous source hashes before updating the WP-04 test workspace."""
import hashlib
import json
import pathlib
import tarfile

root = pathlib.Path.cwd().resolve()
assert str(root) == "/home/gabriele/rust-develop/minimalMonitor/validation/wp04/workspace"
old_path = root / "installed-manifest.json"
old = json.loads(old_path.read_text()) if old_path.exists() else {}
manifest = json.loads((root / "transfer-manifest.json").read_text())
with tarfile.open(root / "source.tar") as archive:
    assert {m.name for m in archive} == set(manifest)
    for member in archive:
        path = root / member.name
        assert path.resolve().is_relative_to(root)
        if path.exists():
            assert hashlib.sha256(path.read_bytes()).hexdigest() in (manifest[member.name], old.get(member.name)), f"Existing changes: {member.name}"
    archive.extractall(root, filter="data")
for relative, expected in manifest.items():
    assert hashlib.sha256((root / relative).read_bytes()).hexdigest() == expected
old_path.write_text(json.dumps(manifest, indent=2)+"\n")
print(f"Verified {len(manifest)} files in isolated WP-04 workspace.")
