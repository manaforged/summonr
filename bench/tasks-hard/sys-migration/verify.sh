#!/usr/bin/env bash
python3 - <<'PY'
import glob, sqlite3
con = sqlite3.connect(":memory:")
con.executescript(open("db/schema.sql").read())
for f in sorted(glob.glob("db/migrations/*.sql")):
    con.executescript(open(f).read())
cols = [r[1] for r in con.execute("PRAGMA table_info(users)")]
assert "display_name" in cols, cols
con.execute("INSERT INTO users (id, email) VALUES (1, 'a@b.c')")
con.execute("UPDATE users SET display_name = NULL WHERE id = 1")
rows = con.execute("SELECT display_name FROM users WHERE id=1").fetchall()
assert rows[0][0] is None
try:
    con.execute("INSERT INTO users (id, email, display_name) VALUES (2,'x@y.z','X')")
except Exception as e:
    raise SystemExit(f"insert with display_name failed: {e}")
print("ok")
PY