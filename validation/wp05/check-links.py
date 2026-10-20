"""Check only WP-05 affected Markdown file links; never traverse excluded folders."""
from pathlib import Path
import re, urllib.parse
selected=['README.md','plan/README.md','plan/agents/work-packages.md','plan/decisions.md','plan/design/gui.md','plan/design/web-api.md','plan/validation/acceptance.md','plan/agents/reports/WP-05.md','plan/agents/reports/WP-05-review.md','validation/wp05/README.md']
count=0
for name in selected:
 p=Path(name)
 for target in re.findall(r'\[[^\]]*\]\(([^\s)]+)\)',p.read_text(encoding='utf-8')):
  if target.startswith(('https://','http://','#')):continue
  part=urllib.parse.unquote(target.split('#',1)[0]).strip('<>')
  assert 'prompts' not in Path(part).parts
  if part:
   assert (p.parent/part).exists(),f'{name}: {target}'
   count+=1
print(f'Validated {count} local links in {len(selected)} affected Markdown files.')
