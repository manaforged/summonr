from pathlib import Path
import sys

p = Path(sys.argv[1])
text = p.read_text()
start = text.find('[patch."https://github.com/manaforged/sigil.git"]')
if start < 0:
    raise SystemExit(0)
rest = text[start:]
nxt = rest.find("\n[", 1)
p.write_text(text[:start] + (rest[nxt + 1 :] if nxt >= 0 else ""))
