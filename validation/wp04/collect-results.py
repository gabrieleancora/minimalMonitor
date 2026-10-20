"""Retain small evidence summaries without credentials or raw request bodies."""
import hashlib
import json
import pathlib
import re

root = pathlib.Path.cwd()
out = root / "validation/wp04/results"
out.mkdir(exist_ok=True)
for platform in ("windows", "fedora"):
    evidence = root / f"validation/wp04/evidence/{platform}"
    if not evidence.exists():
        continue
    exits = {}
    for line in (evidence / "exits.tsv").read_text(encoding="utf-8-sig").splitlines():
        if line.strip():
            name, status = line.split("\t")
            exits[name] = int(status)
    suites = []
    for name in ("test",):
        text = (evidence / f"{name}.log").read_text(encoding="utf-8-sig")
        suites.extend({"passed": int(a), "failed": int(b), "ignored": int(c)} for a, b, c in re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", text))
    environment = (evidence / "environment.txt").read_text(encoding="utf-8-sig").splitlines()
    sources = json.loads((root / "validation/wp04/transfer-manifest.json").read_text(encoding="utf-8")) if (root / "validation/wp04/transfer-manifest.json").exists() else {}
    summary = {"package":"WP-04", "platform":platform, "environment":environment, "exits":exits, "test_suites":suites, "passed_total":sum(s["passed"] for s in suites), "source_hashes":sources,
        "log_sha256":{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in evidence.iterdir() if p.is_file()},
        "limits":["Controlled fixtures only; no production token/channel messages", "No sustained memory/disk qualification", "Fedora is not Debian 12 compatibility evidence"]}
    (out / f"{platform}.json").write_text(json.dumps(summary, indent=2)+"\n", encoding="utf-8")
    print(f"{platform}: {summary['passed_total']} passes, {exits}")
