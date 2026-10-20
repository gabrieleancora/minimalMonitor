"""Review the exact Cargo lockfile against OSV; keep raw evidence and licenses.

Run in the repository root. No shared tools are installed. This checks known
Cargo advisories, not unknown vulnerabilities or all native-library CVEs.
"""
import datetime
import json
import pathlib
import subprocess
import sys
import tomllib
import urllib.request


def main():
    root = pathlib.Path.cwd()
    output = root / "validation/wp03/evidence/dependencies"
    output.mkdir(parents=True, exist_ok=True)
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    packages = [p for p in lock["package"] if p.get("source", "").startswith("registry+")]
    queries = [{"package": {"name": p["name"], "ecosystem": "crates.io"}, "version": p["version"]} for p in packages]
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], text=True))
    inventory = [{"name": p["name"], "version": p["version"], "license": p["license"], "source": p["source"]} for p in metadata["packages"]]
    (output / "licenses.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    request = urllib.request.Request("https://api.osv.dev/v1/querybatch", data=json.dumps({"queries": queries}).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    (output / "osv.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    if len(result.get("results", [])) != len(packages):
        raise RuntimeError("incomplete OSV result")
    findings = []
    for package, entry in zip(packages, result["results"], strict=True):
        if entry.get("next_page_token"):
            raise RuntimeError("paginated OSV result requires further review")
        for vulnerability in entry.get("vulns", []):
            findings.append({"name": package["name"], "version": package["version"], "id": vulnerability["id"]})
    summary = {"checked_at_utc": datetime.datetime.now(datetime.UTC).isoformat(), "packages": len(packages), "findings": findings, "license_expressions": sorted({p["license"] or "UNSPECIFIED" for p in inventory if p["source"]})}
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
