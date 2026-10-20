"""Package only compile inputs and controlled runners; no configuration or owner folders."""
import hashlib, io, json, pathlib, tarfile, time
root = pathlib.Path.cwd()
files=[pathlib.Path(p) for p in ('Cargo.toml','Cargo.lock','rust-toolchain.toml')]
for folder in ('src','tests','migrations','assets'):
    files.extend(p.relative_to(root) for p in (root/folder).rglob('*') if p.is_file())
files.extend(pathlib.Path(p) for p in ('validation/wp05/fixture.rs','validation/wp05/run-linux.sh','validation/wp04/lifecycle.py'))
manifest={}
with tarfile.open('validation/wp05/source.tar','w') as archive:
    for relative in files:
        data=(root/relative).read_bytes().replace(b'\r\n',b'\n')
        manifest[relative.as_posix()]=hashlib.sha256(data).hexdigest()
        info=tarfile.TarInfo(relative.as_posix());info.size=len(data);info.mode=0o644;info.mtime=time.time()
        archive.addfile(info,io.BytesIO(data))
pathlib.Path('validation/wp05/transfer-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
print(f'Packed {len(files)} selected compile inputs/runners. Excluded configuration, credentials and owner folders.')
