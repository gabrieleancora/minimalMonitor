"""Check file links only in the WP-04 affected specifications/reports."""
import pathlib
import re
import urllib.parse

root = pathlib.Path.cwd()
selected = ["README.md", "plan/README.md", "plan/decisions.md", "plan/agents/work-packages.md",
    "plan/agents/reports/WP-04.md", "plan/agents/reports/WP-04-review.md", "plan/design/architecture.md", "plan/design/data-model.md",
    "plan/design/web-api.md", "plan/design/notifications.md", "plan/operations/security.md",
    "plan/validation/acceptance.md", "validation/wp04/README.md"]
checked = 0
errors = []
for relative in selected:
    source = root / relative
    text = source.read_text(encoding="utf-8")
    for link in re.findall(r"\[[^\]]*\]\(([^\s)]+)\)", text):
        if link.startswith(("https://", "http://", "#")):
            continue
        path = urllib.parse.unquote(link.split("#", 1)[0]).strip("<>")
        if not path:
            continue
        assert "prompts" not in pathlib.PurePath(path).parts
        checked += 1
        if not (source.parent / path).exists():
            errors.append(f"{relative}: {link}")
assert not errors, "\n".join(errors)
print(f"Validated {checked} local file links across {len(selected)} affected Markdown files.")
