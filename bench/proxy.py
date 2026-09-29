#!/usr/bin/env python3
import argparse
import json
import re
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DROP = {"host", "content-length", "accept-encoding", "connection", "transfer-encoding"}


STALL_SECONDS = 120
NUDGE = "reasoned at length without acting"
TAIL_BYTES = 4096


def role_sizes(body, messages):
    roles = {}
    if body.get("system"):
        roles["system"] = {"count": 1, "bytes": len(json.dumps(body["system"], ensure_ascii=False))}
    for message in messages:
        size = len(json.dumps(message.get("content"), ensure_ascii=False))
        size += len(json.dumps(message.get("tool_calls") or [], ensure_ascii=False))
        entry = roles.setdefault(message.get("role", "unknown"), {"count": 0, "bytes": 0})
        entry["count"] += 1
        entry["bytes"] += size
    return roles


def last_round(messages):
    last = max((i for i, m in enumerate(messages) if m.get("role") == "assistant"), default=None)
    if last is None:
        return [], []
    calls = [
        {"name": (call.get("function") or {}).get("name"), "args": ((call.get("function") or {}).get("arguments") or "")[:240]}
        for call in messages[last].get("tool_calls") or []
    ]
    results = [
        {"role": message.get("role"), "bytes": len(json.dumps(message.get("content"), ensure_ascii=False))}
        for message in messages[last + 1:]
    ]
    return calls, results


def shape(body):
    messages = body.get("messages") or []
    calls, results = last_round(messages)
    tools = body.get("tools") or []
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
        "nudged": NUDGE in json.dumps(messages[-1:], ensure_ascii=False),
        "roles": role_sizes(body, messages),
        "tools": len(tools),
        "tools_bytes": len(json.dumps(tools, ensure_ascii=False)),
    }


def normalize(usage):
    if not usage or "input_tokens" not in usage and "output_tokens" not in usage:
        return usage
    read = usage.get("cache_read_input_tokens") or 0
    prompt = (usage.get("input_tokens") or 0) + read + (usage.get("cache_creation_input_tokens") or 0)
    return {
        **usage,
        "prompt_tokens": prompt,
        "completion_tokens": usage.get("output_tokens") or 0,
        "prompt_tokens_details": {"cached_tokens": read},
    }


def stream_chunks(raw):
    for line in raw.decode("utf-8", "replace").splitlines():
        data = line[5:].strip() if line.startswith("data:") else ""
        if not data or data == "[DONE]":
            continue
        try:
            yield json.loads(data)
        except json.JSONDecodeError:
            continue


def merge_usage(usage, chunk):
    message = chunk.get("message") or {}
    if message.get("usage"):
        usage = {**(usage or {}), **message["usage"]}
    if chunk.get("usage"):
        usage = {**usage, **chunk["usage"]} if usage and chunk.get("type") == "message_delta" else chunk["usage"]
    return usage


def finish_of(chunk, finish):
    finish = (chunk.get("delta") or {}).get("stop_reason") or finish
    for choice in chunk.get("choices") or []:
        finish = choice.get("finish_reason") or finish
    return finish


def usage_from_stream(raw):
    usage, model, finish = None, None, None
    for chunk in stream_chunks(raw):
        model = chunk.get("model") or (chunk.get("message") or {}).get("model") or model
        usage = merge_usage(usage, chunk)
        finish = finish_of(chunk, finish)
    return normalize(usage), model, finish


def stall_tail(raw, latency, error):
    if latency < STALL_SECONDS and not error:
        return None
    return {"bytes": len(raw), "last": raw[-TAIL_BYTES:].decode("utf-8", "replace")}


def usage_from_body(raw):
    try:
        body = json.loads(raw)
    except json.JSONDecodeError:
        return None, None, None
    choices = body.get("choices") or [{}]
    finish = choices[0].get("finish_reason") or body.get("stop_reason")
    return normalize(body.get("usage")), body.get("model"), finish


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

    def route(self):
        parts = self.path.split("/", 2)
        labeled = len(parts) > 2 and parts[1] != "v1"
        label = parts[1] if labeled else "unlabeled"
        path = "/" + parts[2] if labeled else self.path
        prefix = next((prefix for prefix in self.strip if path.startswith(prefix + "/")), "")
        return label, path[len(prefix):]

    def prepare(self, raw, label, path):
        if not raw or not (path.endswith("/chat/completions") or path.endswith("/messages")):
            return raw, None
        body = json.loads(raw)
        record = {"label": label, "request": shape(body), "usage_requested": False}
        options = body.get("stream_options") or {}
        if path.endswith("/chat/completions") and body.get("stream") and not options.get("include_usage"):
            body["stream_options"] = {**options, "include_usage": True}
            record["usage_requested"] = True
            raw = json.dumps(body).encode()
        return raw, record

    def target(self, path):
        if path.startswith("/anthropic") and self.anthropic:
            return self.anthropic + path[len("/anthropic") :]
        if re.search(r"/v\d+$", self.upstream) and path.startswith("/v1/"):
            return self.upstream + path[len("/v1") :]
        return self.upstream + path

    def stream(self, response):
        first, chunks = None, []
        try:
            while chunk := (response.read1(65536) if hasattr(response, "read1") else response.read(65536)):
                first = first or time.time()
                chunks.append(chunk)
                self.wfile.write(chunk)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError) as error:
            return first, chunks, f"cut off: {type(error).__name__}"
        return first, chunks, None

    def relay(self, method):
        label, path = self.route()
        length = int(self.headers.get("Content-Length") or 0)
        try:
            raw, record = self.prepare(self.rfile.read(length) if length else None, label, path)
        except json.JSONDecodeError as error:
            self.send_error(400, f"request body is not JSON: {error}")
            return
        headers = {k: v for k, v in self.headers.items() if k.lower() not in DROP}
        if "opencode.ai" in self.upstream:
            headers["x-opencode-session"] = label
        request = urllib.request.Request(self.target(path), data=raw, method=method, headers=headers)
        started = time.time()
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
        first, chunks, cut = self.stream(response)
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
                "tail": stall_tail(raw, ended - started, error),
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
