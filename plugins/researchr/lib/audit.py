import enum
import hashlib
import json
import time
from dataclasses import dataclass, replace

def text(value, label):
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a nonempty string")
    return value


class Source(enum.StrEnum):
    SESSION = "session"
    MAIL = "mail"
    PROVIDER = "provider"
    EXPORT = "export"


@dataclass(frozen=True)
class Record:
    session: str
    source: Source
    stream: str
    sequence: int
    kind: str
    at: int | None
    body: str
    sha256: str

    @classmethod
    def parse(cls, raw):
        row = json.loads(raw)
        if type(row.get("schema")) is not int or row["schema"] != 1:
            raise ValueError("unsupported audit schema")
        try:
            source = Source(row["source"])
        except ValueError as error:
            raise ValueError("unsupported audit source") from error
        sequence = row["sequence"]
        if type(sequence) is not int or sequence < 1:
            raise ValueError("audit sequence must be a positive integer")
        data = row["record"]
        if not isinstance(data, dict):
            raise ValueError("audit record must be an object")
        at = data.get("at")
        if at is not None and type(at) is not int:
            raise ValueError("audit timestamp must be an integer or absent")
        stream = row.get("stream", "")
        if not isinstance(stream, str):
            raise ValueError("audit stream must be a string")
        body = raw.decode("utf-8").rstrip("\n")
        return cls(text(row["session"], "audit session"), source,
                   stream, sequence, text(data["kind"], "audit kind"),
                   at, body, hashlib.sha256(body.encode()).hexdigest())


def commit(store, batch):
    db = store.db
    db.execute("begin immediate")
    inserted = 0
    added = 0
    try:
        for row in batch:
            key = (row.session, row.source, row.stream, row.sequence)
            prior = db.execute("select sha256 from event where session=? and source=? and stream=? and sequence=?", key).fetchone()
            if prior is not None:
                if prior[0] != row.sha256:
                    raise ValueError("audit history changed at an existing sequence; refusing overwrite")
                continue
            added += len(row.body.encode())
            db.execute("insert into event values(?,?,?,?,?,?,?,?,?)",
                       (*key, row.kind, row.at, row.body, row.sha256, time.time()))
            inserted += 1
        used = db.execute("select bytes from audit_size where id=1").fetchone()[0]
        if used + added > store.limits.audit_bytes:
            raise ValueError("audit byte budget exhausted; no records from this batch were committed")
        db.execute("update audit_size set bytes=bytes+? where id=1", (added,))
        db.commit()
        return inserted
    except BaseException:
        db.rollback()
        raise


def records(source, limit):
    while raw := source.readline(limit + 1):
        if len(raw) > limit or not raw.endswith(b"\n"):
            raise ValueError("oversize or incomplete audit record; replay the complete source")
        yield Record.parse(raw)


def import_records(store, source, allow_partial, expected_session=None):
    batch = []
    size = 0
    total = 0
    inserted = 0
    complete = False
    seen = {}
    limits = store.limits
    for row in records(source, limits.record_bytes):
        if expected_session is not None and row.session != expected_session:
            raise ValueError("audit record belongs to a different session")
        if complete:
            raise ValueError("records follow the audit export trailer")
        if row.source == Source.EXPORT:
            manifest = json.loads(row.body)["record"]
            if manifest.get("records") != total or manifest.get("kind") != "complete_prefix":
                raise ValueError("audit export count mismatch")
            complete = True
            batch.append(replace(row, stream=row.sha256))
            continue
        key = (row.session, row.source, row.stream)
        expected = seen.get(key, 0) + 1
        if row.sequence != expected:
            raise ValueError("audit stream has a sequence gap or does not start at one")
        seen[key] = row.sequence
        batch.append(row)
        size += len(row.body.encode())
        total += 1
        if len(batch) >= limits.page_size or size >= limits.batch_bytes:
            inserted += commit(store, batch)
            batch, size = [], 0
    if total == 0:
        raise ValueError("no audit records")
    if not complete and not allow_partial:
        raise ValueError("audit export trailer missing; earlier batches may be committed; replay or explicitly allow partial capture")
    inserted += commit(store, batch)
    envelopes = total + int(complete)
    return {"records": total, "envelopes": envelopes, "inserted": inserted, "existing": envelopes - inserted,
            "complete_export": complete, "streams": len(seen),
            "sessions": sorted({key[0] for key in seen})}


def query(store, session, offset, full):
    if offset < 0:
        raise ValueError("offset must be nonnegative")
    where = " where session=?" if session else ""
    params = [session] if session else []
    rows = store.db.execute("select * from event" + where +
                            " order by session,source,stream,sequence limit ? offset ?",
                            (*params, store.limits.page_size, offset))
    events = []
    budget = store.limits.text_bytes
    for row in rows:
        item = dict(row)
        body = item.pop("body")
        if full and len(body.encode()) <= budget:
            item["record"] = json.loads(body)
            budget -= len(body.encode())
        elif full:
            item["omitted"] = "record exceeds output budget; full body remains in SQLite event.body"
        events.append(item)
    return {"events": events, "next_offset": offset + len(events) if len(events) == store.limits.page_size else None,
            "audit_bytes": store.db.execute("select bytes from audit_size where id=1").fetchone()[0]}
