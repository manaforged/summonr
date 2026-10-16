#!/usr/bin/env python3
import argparse
import http.client
import os
import subprocess
import json
import re
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import upstream

DROP = {"host", "content-length", "accept-encoding", "connection", "transfer-encoding"}


STALL_SECONDS = 120
NUDGE = "reasoned at length without acting"
TAIL_BYTES = 4096
ACTIVE_SECONDS = 1800


def role_sizes(body, messages):
    roles = {}
    system = body.get("system") or body.get("instructions")
    if system:
        roles["system"] = {"count": 1, "bytes": len(json.dumps(system, ensure_ascii=False))}
    for message in messages:
        size = len(json.dumps(message.get("content") if "content" in message else message, ensure_ascii=False))
        size += len(json.dumps(message.get("tool_calls") or [], ensure_ascii=False))
        entry = roles.setdefault(message.get("role") or message.get("type", "unknown"), {"count": 0, "bytes": 0})
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


def item_round(items):
    start = len(items)
    while start and items[start - 1].get("type", "").endswith(("_call", "_call_output", "reasoning")):
        start -= 1
    tail = items[start:]
    calls = [{"name": item.get("name"), "args": (item.get("arguments") or item.get("input") or "")[:240]} for item in tail if item.get("type", "").endswith("_call")]
    results = [{"role": item.get("type"), "bytes": len(json.dumps(item.get("output"), ensure_ascii=False))} for item in tail if item.get("type", "").endswith("_output")]
    return calls, results


def shape(body):
    items = body.get("input") or []
    items = items if isinstance(items, list) else [{"role": "user", "content": items}]
    messages = body.get("messages") or items
    calls, results = last_round(messages) if "messages" in body else item_round(items)
    tools = body.get("tools") or [tool for item in items if item.get("type") == "additional_tools" for tool in item.get("tools") or []]
    return {
        "round_calls": calls,
        "round_results": results,
        "model": body.get("model"),
        "stream": bool(body.get("stream")),
        "temperature": body.get("temperature"),
        "top_p": body.get("top_p"),
        "max_tokens": body.get("max_tokens") or body.get("max_completion_tokens") or body.get("max_output_tokens"),
        "reasoning_effort": body.get("reasoning_effort") or (body.get("reasoning") or body.get("output_config") or {}).get("effort"),
        "thinking": body.get("thinking"),
        "tool_stream": body.get("tool_stream"),
        "messages": len(messages),
        "nudged": NUDGE in json.dumps(messages[-1:], ensure_ascii=False),
        "roles": role_sizes(body, messages),
        "tools": len(tools),
        "tools_bytes": len(json.dumps(tools, ensure_ascii=False)),
    }


def responses_usage(usage):
    return {
        **usage,
        "prompt_tokens": usage.get("input_tokens") or 0,
        "completion_tokens": usage.get("output_tokens") or 0,
        "prompt_tokens_details": usage.get("input_tokens_details") or {},
        "completion_tokens_details": usage.get("output_tokens_details") or {},
    }


def anthropic_usage(usage):
    read = usage.get("cache_read_input_tokens") or 0
    prompt = (usage.get("input_tokens") or 0) + read + (usage.get("cache_creation_input_tokens") or 0)
    return {
        **usage,
        "prompt_tokens": prompt,
        "completion_tokens": usage.get("output_tokens") or 0,
        "prompt_tokens_details": {"cached_tokens": read},
        "completion_tokens_details": {"reasoning_tokens": (usage.get("output_tokens_details") or {}).get("thinking_tokens")},
    }


def fold_reasoning(usage):
    reasoning = ((usage or {}).get("completion_tokens_details") or {}).get("reasoning_tokens") or 0
    if not reasoning or usage.get("total_tokens") != (usage.get("prompt_tokens") or 0) + (usage.get("completion_tokens") or 0) + reasoning:
        return usage
    return {**usage, "completion_tokens": (usage.get("completion_tokens") or 0) + reasoning, "reasoning_folded": reasoning}


def normalize(usage):
    if not usage or "input_tokens" not in usage and "output_tokens" not in usage:
        return usage
    anthropic = "cache_read_input_tokens" in usage or "cache_creation_input_tokens" in usage
    if not anthropic and ("input_tokens_details" in usage or "output_tokens_details" in usage):
        return responses_usage(usage)
    return anthropic_usage(usage)


def decoded(raw, encoding):
    if encoding != "zstd":
        return raw
    try:
        from compression import zstd
    except ImportError:
        return subprocess.run(["zstd", "-dcq"], input=raw, capture_output=True, check=True).stdout
    return zstd.decompress(raw)


def stream_chunks(raw):
    for line in raw.decode("utf-8", "replace").splitlines():
        data = line[5:].strip() if line.startswith("data:") else ""
        if not data or data == "[DONE]":
            continue
        try:
            yield json.loads(data)
        except json.JSONDecodeError:
            continue


def frame_body(chunk):
    body = chunk.get("message") or chunk.get("response")
    return body if isinstance(body, dict) else {}


def merge_usage(usage, chunk):
    message = frame_body(chunk)
    if message.get("usage"):
        usage = {**(usage or {}), **message["usage"]}
    if chunk.get("usage"):
        usage = {**usage, **chunk["usage"]} if usage and chunk.get("type") == "message_delta" else chunk["usage"]
    return usage


def finish_of(chunk, finish):
    delta = chunk.get("delta") if isinstance(chunk.get("delta"), dict) else {}
    finish = delta.get("stop_reason") or frame_body(chunk).get("status") or finish
    for choice in chunk.get("choices") or []:
        finish = choice.get("finish_reason") or finish
    return finish


def usage_from_stream(raw):
    usage, model, finish = None, None, None
    for chunk in stream_chunks(raw):
        model = chunk.get("model") or frame_body(chunk).get("model") or model
        usage = merge_usage(usage, chunk)
        finish = finish_of(chunk, finish)
    return fold_reasoning(normalize(usage)), model, finish


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
    finish = choices[0].get("finish_reason") or body.get("stop_reason") or body.get("status")
    return fold_reasoning(normalize(body.get("usage"))), body.get("model"), finish


def cached(usage):
    if not usage:
        return None
    if "prompt_cache_hit_tokens" in usage:
        return usage["prompt_cache_hit_tokens"]
    return (usage.get("prompt_tokens_details") or {}).get("cached_tokens")


class Proxy(BaseHTTPRequestHandler):
    upstream = ""
    routes = {}
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
        group, _, label = (parts[1] if labeled else "unlabeled").rpartition("~")
        path = "/" + parts[2] if labeled else self.path
        prefix = next((prefix for prefix in self.strip if path.startswith(prefix + "/")), "")
        return group or label, label, path[len(prefix):]

    def key_for(self, group):
        now = time.time()
        with self.lock:
            index = self.groups.get(group, (None, 0))[0]
            if index is None:
                load = [0] * len(self.keys)
                for slot, seen in self.groups.values():
                    if now - seen < ACTIVE_SECONDS:
                        load[slot] += 1
                index = load.index(min(load))
            self.groups[group] = (index, now)
        return index

    def prepare(self, raw, label, path, index, url, encoding):
        if not raw or not path.partition("?")[0].endswith(("/chat/completions", "/messages", "/responses")):
            return raw, None
        body = json.loads(decoded(raw, encoding))
        host = urllib.parse.urlsplit(url).hostname
        dropped = [field for field in self.drops.get(host, ()) if field in body]
        record = {"label": label, "key": index, "upstream": host, "dropped": dropped, "request": shape(body), "usage_requested": False}
        for field in dropped:
            del body[field]
        options = body.get("stream_options") or {}
        if path.endswith("/chat/completions") and body.get("stream") and not options.get("include_usage"):
            body["stream_options"] = {**options, "include_usage": True}
            record["usage_requested"] = True
        if encoding:
            record["dropped"], record["usage_requested"] = [], False
        elif dropped or record["usage_requested"]:
            raw = json.dumps(body).encode()
        return raw, record

    def target(self, path, index):
        upstream = self.upstreams[index]
        for name, url in self.routes.items():
            if path.startswith(f"/{name}/"):
                return url + path[len(name) + 1 :]
        if re.search(r"/v\d+$", upstream) and path.startswith("/v1/"):
            return upstream + path[len("/v1") :]
        return upstream + path

    def stream(self, response):
        first, chunks = None, []
        try:
            while chunk := (response.read1(65536) if hasattr(response, "read1") else response.read(65536)):
                first = first or time.time()
                chunks.append(chunk)
                self.wfile.write(chunk)
                self.wfile.flush()
        except (OSError, http.client.HTTPException) as error:
            return first, chunks, f"cut off: {type(error).__name__}"
        return first, chunks, None

    def dump_body(self, raw, record, label):
        if self.dump and record is not None:
            folder = os.path.join(self.dump, label)
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, f"{time.time_ns()}.json"), "wb") as out:
                out.write(raw)

    def upstream_headers(self, key, index, label):
        headers = {k: v for k, v in self.headers.items() if k.lower() not in DROP}
        for name in headers:
            if key is None:
                break
            if name.lower() == "authorization":
                headers[name] = f"Bearer {key}"
            elif name.lower() == "x-api-key":
                headers[name] = key
        if "opencode.ai" in self.upstreams[index]:
            headers["x-opencode-session"] = label
        return headers

    def answer(self, response, status, record, started):
        self.send_response(status)
        for key, value in response.headers.items():
            if key.lower() not in DROP:
                self.send_header(key, value)
        self.send_header("Connection", "close")
        self.end_headers()
        first, chunks, cut = self.stream(response)
        self.write(record, started, first, status, b"".join(chunks), cut)

    def relay(self, method):
        group, label, path = self.route()
        index = self.key_for(group)
        key = self.keys[index]
        length = int(self.headers.get("Content-Length") or 0)
        try:
            url = self.target(path, index)
            encoding = (self.headers.get("Content-Encoding") or "").strip().lower()
            raw, record = self.prepare(self.rfile.read(length) if length else None, label, path, index, url, encoding)
        except (json.JSONDecodeError, UnicodeDecodeError, ValueError, subprocess.CalledProcessError) as error:
            self.send_error(400, f"request body is not JSON: {error}")
            return
        self.dump_body(raw, record, label)
        headers = self.upstream_headers(key, index, label)
        request = urllib.request.Request(url, data=raw, method=method, headers=headers)
        started = time.time()
        try:
            response = self.opener.open(request, timeout=upstream.READ_SECONDS)
            status = response.status
        except urllib.error.HTTPError as error:
            response, status = error, error.code
        except (OSError, http.client.HTTPException) as error:
            reason = str(getattr(error, "reason", None) or f"{type(error).__name__}: {error}")
            self.write(record, started, None, 502, b"", reason)
            self.send_error(502, reason)
            return
        self.answer(response, status, record, started)

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
    parser.add_argument("--route", action="append", default=[])
    parser.add_argument("--log", required=True)
    parser.add_argument("--strip", action="append", default=[])
    parser.add_argument("--key-env", action="append", default=[])
    parser.add_argument("--dump")
    parser.add_argument("--drop", action="append", default=[])
    parser.add_argument("--connect-timeout", type=float, default=30.0)
    args = parser.parse_args()
    routes = [entry.partition("=") for entry in args.key_env]
    Proxy.keys = [os.environ.get(name) or parser.error(f"{name} is not set") for name, _, _ in routes] or [None]
    Proxy.upstreams = [(url or args.upstream).rstrip("/") for _, _, url in routes] or [args.upstream.rstrip("/")]
    Proxy.groups = {}
    Proxy.drops = {}
    for entry in args.drop:
        host, _, fields = entry.partition("=")
        Proxy.drops.setdefault(host, []).extend(filter(None, fields.split(",")))
    Proxy.dump = args.dump
    Proxy.opener = upstream.opener(args.connect_timeout)
    Proxy.upstream = args.upstream.rstrip("/")
    Proxy.routes = {name: url.rstrip("/") for name, _, url in (entry.partition("=") for entry in args.route)}
    Proxy.strip = [prefix.rstrip("/") for prefix in args.strip]
    Proxy.log = open(args.log, "a")
    ThreadingHTTPServer((args.bind, args.port), Proxy).serve_forever()


if __name__ == "__main__":
    main()
