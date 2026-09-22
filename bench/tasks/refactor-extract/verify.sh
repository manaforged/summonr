cd app && python3 -m unittest discover -s . 2>/dev/null \
  && test -f text.py \
  && python3 -c "from text import slug, title_of" \
  && ! grep -n "def slug\|def title_of" main.py
