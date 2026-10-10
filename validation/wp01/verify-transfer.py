"""Compare previously transferred source bytes without Windows tar UID/mode noise."""
import pathlib
import sys
import tarfile

root = pathlib.Path.cwd().resolve()
with tarfile.open(sys.argv[1]) as archive:
    for member in archive.getmembers():
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or "prompts" in path.parts:
            raise RuntimeError("invalid source archive path")
        destination = root.joinpath(*path.parts)
        if destination.is_symlink():
            raise RuntimeError("source path became a symlink")
        if member.isdir():
            assert destination.is_dir(), f"directory missing: {member.name}"
        elif member.isfile() and member.size <= 1024 * 1024:
            with archive.extractfile(member) as source:
                assert source.read() == destination.read_bytes(), f"source changed: {member.name}"
        else:
            raise RuntimeError("unsupported archive entry")
print("Previously transferred source bytes unchanged.")
