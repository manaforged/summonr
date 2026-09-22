def handle(payload):
    # v2 adds optional "meta" — but legacy callers omit it AND omit "ts"
    return {
        "ok": True,
        "user": payload["user"],
        "ts": payload["ts"],
        "meta": payload.get("meta", {}),
        "v": 2,
    }
