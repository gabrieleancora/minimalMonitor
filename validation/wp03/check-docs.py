"""Check local links in the plan and assigned documentation only."""
import pathlib
import re
import urllib.parse

files=list(pathlib.Path("plan").rglob("*.md"))+[
    pathlib.Path("README.md"),pathlib.Path("AGENTS.md"),pathlib.Path("validation/wp03/README.md")]
checked=0
missing=[]
for source in files:
    for match in re.finditer(r"(?<!!)\[[^\]]*\]\(([^)]+)\)",source.read_text(encoding="utf-8-sig")):
        link=match.group(1).strip().split(' "')[0].strip("<>")
        if "://" in link or link.startswith(("#","mailto:")):
            continue
        target=urllib.parse.unquote(link.split("#")[0])
        if not target or "prompts" in pathlib.PurePath(target).parts:
            continue
        checked+=1
        if not (source.parent/target).exists():
            missing.append((str(source),target))
print({"checked":checked,"missing":missing})
assert not missing
