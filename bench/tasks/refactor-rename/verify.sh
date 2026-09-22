cd app && python3 -m unittest discover -s . 2>/dev/null && ! grep -rn "fetch_url" --include="*.py" . | grep -v "^Binary"
