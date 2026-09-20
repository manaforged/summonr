#!/usr/bin/env python3
import argparse
import json
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DROP = {"host", "content-length", "accept-encoding", "connection", "transfer-encoding"}


def shape(body):
    messages = body.get("messages") or []
    roles = {}
    for message in messages:
        role = message.get("role", "unknown")
        size = len(json.dumps(message.get("content"), ensure_ascii=False))
        size += len(json.dumps(message.get("tool_calls") or [], ensure_ascii=False))
        entry = roles.setdefault(role, {"count": 0, "bytes": 0})
        entry["count"] += 1
        entry["bytes"] += size
    last = max((i for i, m in enumerate(messages) if m.get("role") == "assistant"), default=None)
    calls, results = [], []
    if last is not None:
        for call in messages[last].get("tool_calls") or []:
            function = call.get("function") or {}
            calls.append({"name": function.get("name"), "args": (function.get("arguments") or "")[:240]})
        for message in messages[last + 1:]:
            results.append({"role": message.get("role"), "bytes": len(json.dumps(message.get("content"), ensure_ascii=False))})
    return {
        "round_calls": calls,
        "round_results": results,
        "model": body.get("model"),
        "stream": bool(body.get("stream")),
        "temperature": body.get("temperature"),
        "top_p": body.get("top_p"),
        "max_tokens": body.get("max_tokens") or body.get("max_completion_tokens"),
        "reasoning_effort": body.get("reasoning_effort"),
        "thinking": body.get("thinking"),
        "tool_stream": body.get("tool_stream"),
        "messages": len(messages),
        "roles": roles,
        "tools": len(body.get("tools") or []),
        "tools_bytes": len(json.dumps(body.get("tools") or [], ensure_ascii=False)),
    }


def usage_from_stream(raw):
    usage, model, finish = None, None, None
    for line in raw.decode("utf-8", "replace").splitlines():
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if not data or data == "[DONE]":
            continue
        try:
            chunk = json.loads(data)
        except json.JSONDecodeError:
            continue
        model = chunk.get("model") or model
        usage = chunk.get("usage") or usage
        for choice in chunk.get("choices") or []:
            finish = choice.get("finish_reason") or finish
    return usage, model, finish


def usage_from_body(raw):
    try:
        body = json.loads(raw)
    except json.JSONDecodeError:
        return None, None, None
    choices = body.get("choices") or [{}]
    return body.get("usage"), body.get("model"), choices[0].get("finish_reason")


def cached(usage):
    if not usage:
        return None
    if "prompt_cache_hit_tokens" in usage:
        return usage["prompt_cache_hit_tokens"]
    return (usage.get("prompt_tokens_details") or {}).get("cached_tokens")


class Proxy(BaseHTTPRequestHandler):
    upstream = ""
    anthropic = ""
    strip = []
    log = None
    lock = threading.Lock()

    def log_message(self, *args):
        pass

    def do_GET(self):
        self.relay("GET")

    def do_POST(self):
        self.relay("POST")

    def relay(self, method):
        parts = self.path.split("/", 2)
        labeled = len(parts) > 2 and parts[1] != "v1"
        label = parts[1] if labeled else "unlabeled"
        path = "/" + parts[2] if labeled else self.path
        for prefix in self.strip:
            if path.startswith(prefix + "/"):
                path = path[len(prefix):]
                break
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else None
        record = None
        if raw and (path.endswith("/chat/completions") or path.endswith("/messages")):
            body = json.loads(raw)
            record = {"label": label, "request": shape(body), "usage_requested": False}
            if path.endswith("/chat/completions") and body.get("stream") and not (body.get("stream_options") or {}).get("include_usage"):
                body["stream_options"] = {**(body.get("stream_options") or {}), "include_usage": True}
                record["usage_requested"] = True
                raw = json.dumps(body).encode()
        headers = {k: v for k, v in self.headers.items() if k.lower() not in DROP}
        if path.startswith("/anthropic") and self.anthropic:
            target = self.anthropic + path[len("/anthropic") :]
        else:
            target = self.upstream + path
        request = urllib.request.Request(target, data=raw, method=method, headers=headers)
        started = time.time()
        first = None
        chunks = []
        try:
            response = urllib.request.urlopen(request, timeout=900)
            status = response.status
        except urllib.error.HTTPError as error:
            response, status = error, error.code
        except urllib.error.URLError as error:
            self.send_error(502, str(error.reason))
            self.write(record, started, None, 502, b"", str(error.reason))
            return
        self.send_response(status)
        for key, value in response.headers.items():
            if key.lower() not in DROP:
                self.send_header(key, value)
        self.send_header("Connection", "close")
        self.end_headers()
        cut = None
        try:
            while True:
                chunk = response.read1(65536) if hasattr(response, "read1") else response.read(65536)
                if not chunk:
                    break
                if first is None:
                    first = time.time()
                chunks.append(chunk)
                self.wfile.write(chunk)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError) as error:
            cut = f"cut off: {type(error).__name__}"
        self.write(record, started, first, status, b"".join(chunks), cut)

    def write(self, record, started, first, status, raw, error):
        if record is None:
            return
        ended = time.time()
        if record["request"]["stream"]:
            usage, model, finish = usage_from_stream(raw)
        else:
            usage, model, finish = usage_from_body(raw)
        record.update(
            {
                "started": started,
                "first_byte": first,
                "ended": ended,
                "latency": ended - started,
                "status": status,
                "response_model": model,
                "finish_reason": finish,
                "usage": usage,
                "cached_tokens": cached(usage),
                "reasoning_tokens": ((usage or {}).get("completion_tokens_details") or {}).get("reasoning_tokens"),
                "cut_off": bool(error) and error.startswith("cut off"),
                "error": error or (raw[:500].decode("utf-8", "replace") if status >= 400 else None),
            }
        )
        with self.lock:
            self.log.write(json.dumps(record) + "\n")
            self.log.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8094)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--upstream", default="https://api.deepseek.com")
    parser.add_argument("--anthropic", default="https://api.deepseek.com/anthropic")
    parser.add_argument("--log", required=True)
    parser.add_argument("--strip", action="append", default=[])
    args = parser.parse_args()
    Proxy.upstream = args.upstream.rstrip("/")
    Proxy.anthropic = args.anthropic.rstrip("/")
    Proxy.strip = [prefix.rstrip("/") for prefix in args.strip]
    Proxy.log = open(args.log, "a")
    ThreadingHTTPServer((args.bind, args.port), Proxy).serve_forever()


if __name__ == "__main__":
    main()
