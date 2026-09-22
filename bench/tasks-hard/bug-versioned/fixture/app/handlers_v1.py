def handle(payload):
    # v1 payloads always carry {"user": name, "ts": int}
    return {"ok": True, "user": payload["user"], "ts": payload["ts"], "v": 1}
