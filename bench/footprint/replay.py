#!/usr/bin/env python3
import argparse
import json
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

USAGE = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}


def expand(steps):
    out = []
    for step in steps:
        if "repeat" in step:
            for _ in range(int(step["repeat"])):
                out.extend(expand(step["steps"]))
        else:
            out.append(step)
    return out


class Script:
    def __init__(self, steps):
        self.steps = expand(steps)
        self.lock = threading.Lock()
        self.served = 0

    def next(self):
        with self.lock:
            index = self.served
            self.served += 1
        if index < len(self.steps):
            return index, self.steps[index]
        return index, {"text": "done"}


def frames(index, step, model):
    base = {"id": f"replay-{index}", "object": "chat.completion.chunk", "model": model}
    if "call" in step:
        call = step["call"]
        delta = {"role": "assistant", "tool_calls": [{
            "index": 0,
            "id": f"call_{index}",
            "type": "function",
            "function": {"name": call["name"], "arguments": json.dumps(call.get("arguments", {}))},
        }]}
        finish = "tool_calls"
    else:
        delta = {"role": "assistant", "content": step["text"]}
        finish = "stop"
    yield {**base, "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}
    yield {**base, "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}
    yield {**base, "choices": [], "usage": USAGE}


def whole(index, step, model):
    message = {"role": "assistant", "content": None}
    finish = "stop"
    if "call" in step:
        call = step["call"]
        message["tool_calls"] = [{
            "id": f"call_{index}",
            "type": "function",
            "function": {"name": call["name"], "arguments": json.dumps(call.get("arguments", {}))},
        }]
        finish = "tool_calls"
    else:
        message["content"] = step["text"]
    return {
        "id": f"replay-{index}",
        "object": "chat.completion",
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
        "usage": USAGE,
    }


def handler(script, model, log):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_):
            pass

        def reply(self, status, body):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path.split("?")[0].endswith("/registry.json"):
                self.reply(200, {})
            elif self.path.split("?")[0].rstrip("/").endswith("/models"):
                self.reply(200, {"object": "list", "data": [{"id": model, "object": "model", "context_length": 262144}]})
            else:
                self.reply(404, {"error": {"message": "not found"}})

        def do_POST(self):
            length = int(self.headers.get("content-length") or 0)
            body = json.loads(self.rfile.read(length) or b"{}")
            if not self.path.split("?")[0].endswith("/chat/completions"):
                self.reply(404, {"error": {"message": "not found"}})
                return
            index, step = script.next()
            time.sleep(float(step.get("delay", 0)))
            if log:
                with open(log, "a") as sink:
                    sink.write(json.dumps({"index": index, "messages": len(body.get("messages") or []), "step": step}) + "\n")
            if not body.get("stream"):
                self.reply(200, whole(index, step, model))
                return
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("cache-control", "no-cache")
            self.send_header("connection", "close")
            self.end_headers()
            for frame in frames(index, step, model):
                self.wfile.write(f"data: {json.dumps(frame)}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
            self.close_connection = True

    return Handler


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--script", required=True)
    parser.add_argument("--port-file", required=True)
    parser.add_argument("--model", default="replay")
    parser.add_argument("--log")
    args = parser.parse_args()
    with open(args.script) as source:
        script = Script(json.load(source))
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler(script, args.model, args.log))
    with open(args.port_file + ".tmp", "w") as sink:
        sink.write(str(server.server_address[1]))
    os.replace(args.port_file + ".tmp", args.port_file)
    server.serve_forever()


if __name__ == "__main__":
    main()
