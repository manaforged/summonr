import enum
import sqlite3
from dataclasses import dataclass
from pathlib import Path


class Schema(enum.IntEnum):
    LEGACY = 1
    ACTOR = 2
    VIEWS = 3
    RUN = 4
    LEDGER = 5
    CLIMB = 6
    CURRENT = 6


@dataclass(frozen=True)
class Limits:
    audit_bytes: int = 1073741824
    record_bytes: int = 16777216
    page_size: int = 20
    batch_bytes: int = 1048576
    text_bytes: int = 8192
    sync_secs: int = 120
    busy_secs: int = 5

    def __post_init__(self):
        if any(type(value) is not int or value < 1 for value in vars(self).values()):
            raise ValueError("audit limits must be positive integers")


MESSAGE = """create view message as select session,sequence,kind as stage,at,
    json_extract(body,'$.record.actor') as actor,
    json_extract(body,'$.record.mail.id') as id,
    json_extract(body,'$.record.mail.from') as sender,
    json_extract(body,'$.record.mail.to') as recipient,
    json_extract(body,'$.record.mail.reply_to') as reply_to,
    json_extract(body,'$.record.mail.ts') as sent_at,
    json_extract(body,'$.record.mail.text') as text
    from event where source='mail';"""

RESPONSE = """create view response as select session,sequence,kind,at,
    json_extract(body,'$.record.model') as model,
    json_extract(body,'$.record.text') as text,
    json_extract(body,'$.record.reasoning') as reasoning,
    json_extract(body,'$.record.usage') as usage
    from event where source='session' and kind in ('assistant','interrupted');"""

TABLES = (
    """create table event(session text not null, source text not null, stream text not null,
                         sequence integer not null, kind text not null, at integer,
                         body text not null, sha256 text not null, imported_at real not null,
                         primary key(session,source,stream,sequence))""",
    "create index event_kind on event(session,kind,at)",
    "create table audit_size(id integer primary key check(id=1), bytes integer not null)",
    "insert into audit_size values(1,0)",
)

ATTEMPT = (
    """create table attempt(id integer primary key, lane text not null, runner text not null,
                           session text not null, kind text not null, gain real, low real, high real,
                           mechanism text not null, file text, log text, at real not null)""",
    "create index attempt_lane on attempt(lane,id)",
)


class Store:
    def __init__(self, path, create=False, limits=None):
        self.path = Path(path).resolve()
        self.limits = limits or Limits()
        self.db = self.connect(create)
        self.db.row_factory = sqlite3.Row
        self.db.execute(f"pragma busy_timeout={self.limits.busy_secs * 1000}")
        self.db.execute("pragma foreign_keys=on")
        self.db.execute("pragma synchronous=full")
        try:
            version = self.db.execute("pragma user_version").fetchone()[0]
            if version == 0 and not create:
                raise ValueError(f"{self.path} holds no run; run researchr climb with a spec")
            if version == 0:
                self.initialize()
            self.upgrade(Schema(self.db.execute("pragma user_version").fetchone()[0]))
        except BaseException:
            self.db.close()
            raise

    def connect(self, create):
        if create:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            try:
                with self.path.open("xb"):
                    self.path.chmod(0o600)
            except FileExistsError:
                pass
        return sqlite3.connect(self.path.as_uri() + "?mode=rw", uri=True,
                               isolation_level=None, timeout=self.limits.busy_secs)

    def initialize(self):
        self.db.execute("begin immediate")
        try:
            if self.db.execute("pragma user_version").fetchone()[0] == 0:
                existing = self.db.execute("select name from sqlite_master where type='table'").fetchall()
                if existing:
                    raise ValueError("database has unversioned tables; refusing to modify it")
                for statement in TABLES + ATTEMPT:
                    self.db.execute(statement)
                self.db.execute(MESSAGE)
                self.db.execute(RESPONSE)
                self.db.execute(f"pragma user_version={Schema.CURRENT.value}")
            self.db.commit()
        except BaseException:
            self.db.rollback()
            raise

    def upgrade(self, schema):
        if schema == Schema.CURRENT:
            return
        self.db.execute("begin immediate")
        try:
            current = Schema(self.db.execute("pragma user_version").fetchone()[0])
            if current != Schema.CURRENT:
                self.db.execute("select count(*) from event").fetchone()
                self.keep_legacy(current)
                for statement in ATTEMPT:
                    self.db.execute(statement)
                self.db.execute("drop view if exists message")
                self.db.execute("drop view if exists response")
                self.db.execute(MESSAGE)
                self.db.execute(RESPONSE)
                self.db.execute(f"pragma user_version={Schema.CURRENT.value}")
            self.db.commit()
        except BaseException:
            self.db.rollback()
            raise

    def keep_legacy(self, schema):
        tables = {row[0] for row in self.db.execute("select name from sqlite_master where type='table'")}
        for name in ("run", "worker", "finding", "dispatch", "truth"):
            if name in tables:
                kept = f"legacy_{name}" if f"legacy_{name}" not in tables else f"legacy{schema.value}_{name}"
                self.db.execute(f"alter table {name} rename to {kept}")
        self.db.execute("drop index if exists finding_worker")
