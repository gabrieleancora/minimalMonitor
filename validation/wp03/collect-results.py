"""Summarize retained, actual runner evidence without source or secret dumps."""
import json
import pathlib
import re

root=pathlib.Path.cwd()
for platform in ("windows","fedora"):
    evidence=root / "validation/wp03/evidence" / platform
    if not (evidence/"exits.tsv").exists():
        continue
    exits={name:int(code) for line in (evidence/"exits.tsv").read_text(encoding="utf-8-sig").splitlines() if line.strip() for name,code in [line.split("\t")]}
    tests=[tuple(map(int,m)) for m in re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored",(evidence/"test.log").read_text(encoding="utf-8-sig"))]
    lifecycle=json.loads((evidence/"lifecycle.log").read_text(encoding="utf-8-sig"))
    summary={"platform":platform,"exits":exits,"tests_passed":sum(t[0] for t in tests),"tests_failed":sum(t[1] for t in tests),"tests_ignored":sum(t[2] for t in tests),"environment":(evidence/"environment.txt").read_text(encoding="utf-8-sig"),"lifecycle":lifecycle}
    if platform=="fedora":
        summary["narrow_ping"]=(evidence/"narrow-ping.log").read_text()
    assert exits and all(code==0 for code in exits.values()) and summary["tests_failed"]==0
    (root / "validation/wp03/results" / f"{platform}.json").write_text(json.dumps(summary,indent=2)+"\n",encoding="utf-8")
    print(f"{platform}: {summary['tests_passed']} passed; {summary['tests_ignored']} ignored; all runner exits zero")
deps=root / "validation/wp03/evidence/dependencies/summary.json"
if deps.exists():
    (root / "validation/wp03/results/dependencies.json").write_bytes(deps.read_bytes())
