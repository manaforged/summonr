import concurrent.futures
import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path


CLI = Path(__file__).resolve().parents[1] / "bin/researchr"


def row(source, sequence, record, session="session-a", stream=""):
    return dict(schema=1, source=source, session=session, stream=stream,
                sequence=sequence, record=record)


def snapshot(records):
    return records + [row("export", 1, {"kind": "complete_prefix", "records": len(records)})]


class Audit(unittest.TestCase):
    def setUp(self):
        home = tempfile.TemporaryDirectory()
        self.addCleanup(home.cleanup)
        self.root = Path(home.name)
        self.db = self.root / "audit.sqlite"
        self.records = [row("session", 1, {"kind": "assistant", "at": 42,
                         "model": "worker-a", "reasoning": "provider summary",
                         "text": "answer", "usage": {"input": 11, "output": 7},
                         "vendor": {"opaque": "preserved"}}),
                        row("mail", 1, {"kind": "delivered", "at": 43,
                         "actor": "session-a", "mail": {"id": "message-a",
                         "from": "session-a", "to": "session-b",
                         "reply_to": "request-a", "text": "evidence ready"}})]

    def invoke(self, *args, records=None, ok=True, limits=()):
        raw = None if records is None else "".join(json.dumps(record) + "\n" for record in records)
        done = subprocess.run([sys.executable, str(CLI), "--db", str(self.db),
                               *map(str, limits), *map(str, args)], input=raw,
                              capture_output=True, text=True)
        self.assertEqual(done.returncode == 0, ok, (args, done.stdout, done.stderr))
        return json.loads(done.stdout if done.returncode == 0 else done.stderr)

    def test_import_creates_the_audit_and_run_tables(self):
        result = self.invoke("import", records=snapshot(self.records))
        self.assertTrue(result["complete_export"])
        with closing(sqlite3.connect(self.db)) as db, db:
            names = {name for name, in db.execute("select name from sqlite_master where type='table'")}
            self.assertEqual(names, {"event", "audit_size", "attempt"})
            self.assertEqual(db.execute("select model,reasoning,usage from response").fetchone(),
                             ("worker-a", "provider summary", '{"input":11,"output":7}'))
            self.assertEqual(db.execute("select sender,recipient,reply_to,actor,stage from message").fetchone(),
                             ("session-a", "session-b", "request-a", "session-a", "delivered"))
            body = json.loads(db.execute("select body from event where source='session'").fetchone()[0])
            self.assertEqual(body["record"]["vendor"], {"opaque": "preserved"})
        help_ = subprocess.run([sys.executable, str(CLI), "--db", str(self.db), "--help"], capture_output=True, text=True)
        self.assertIn("{climb,submit,status,import,audit}", help_.stdout)
        self.assertNotIn("claim", help_.stdout)

    def test_exact_replay_is_deduplicated_and_changed_history_is_refused(self):
        self.invoke("import", records=snapshot(self.records))
        replay = self.invoke("import", records=snapshot(self.records))
        self.assertEqual((replay["inserted"], replay["existing"]), (0, 3))
        changed = json.loads(json.dumps(self.records))
        changed[0]["record"]["reasoning"] = "overwritten"
        self.invoke("import", records=snapshot(changed), ok=False)
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertEqual(db.execute("select reasoning from response").fetchone()[0], "provider summary")

    def test_missing_manifest_gap_and_empty_export_fail(self):
        cases = [self.records, snapshot([self.records[0] | {"sequence": 2}]), []]
        for records in cases:
            with self.subTest(records=records):
                self.invoke("import", records=records, ok=False)

    def test_partial_provider_capture_is_explicit_and_lossless(self):
        capture = row("provider", 1, {"kind": "chunk", "at": 44,
                      "encoding": "base64", "body": "8J8="}, stream="exchange-a")
        result = self.invoke("import", "--allow-partial", records=[capture])
        self.assertFalse(result["complete_export"])
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertEqual(json.loads(db.execute("select body from event").fetchone()[0])["record"]["body"], "8J8=")

    def test_payload_limit_rolls_back_without_deleting_previous_evidence(self):
        self.invoke("import", records=snapshot(self.records))
        with closing(sqlite3.connect(self.db)) as db, db:
            before = db.execute("select body,sha256 from event order by source").fetchall()
        extra = row("session", 1, {"kind": "assistant", "text": "other"}, session="session-b")
        self.invoke("import", "--allow-partial", records=[extra], limits=("--max-bytes", 1), ok=False)
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertEqual(db.execute("select body,sha256 from event order by source").fetchall(), before)

    def test_concurrent_imports_have_one_copy_of_each_event(self):
        for inserted in (3, 0):
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                results = list(pool.map(lambda _: self.invoke("import", records=snapshot(self.records)), range(8)))
            self.assertEqual(sum(result["inserted"] for result in results), inserted)
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertEqual(db.execute("select count(*) from event").fetchone()[0], 3)

    def test_legacy_coordination_tables_are_kept_beside_the_run_tables(self):
        self.invoke("import", records=snapshot(self.records))
        with closing(sqlite3.connect(self.db)) as db, db:
            before = db.execute("select body,sha256 from event order by source").fetchall()
            db.execute("drop table attempt")
            db.execute("create table run(note text)")
            db.execute("insert into run values('historical control record')")
            db.execute("drop view message")
            db.execute("create view message as select session from event where source='mail'")
            db.execute("pragma user_version=1")
        self.invoke("audit")
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertEqual(db.execute("pragma user_version").fetchone()[0], 6)
            self.assertEqual(db.execute("select actor from message").fetchone()[0], "session-a")
            self.assertEqual(db.execute("select note from legacy_run").fetchone()[0], "historical control record")
            self.assertEqual(db.execute("select body,sha256 from event order by source").fetchall(), before)

    def test_actor_missing_from_legacy_message_stays_unknown(self):
        legacy = self.records[1]
        legacy["record"].pop("actor")
        self.invoke("import", records=snapshot([legacy]))
        with closing(sqlite3.connect(self.db)) as db, db:
            self.assertIsNone(db.execute("select actor from message").fetchone()[0])


if __name__ == "__main__":
    unittest.main()
