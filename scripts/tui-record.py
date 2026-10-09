#!/usr/bin/env python3
"""Record a real summonr session in a pty as an asciicast and cell-grid stills.

usage: tui-record.py PLAN.json

The plan names the working directory, the argv, and the steps. Each step
may type text (one character every "delay" seconds), send keys (escapes such as \\r or \\x1b), wait until the
screen is quiet or the program exits, and dump the screen as the cell grid
that render-splash.py draws:

    {"cwd": "/path/demo", "argv": ["summonr", "zai"], "cast": "/tmp/run.cast",
     "steps": [{"type": "fix the tests"}, {"keys": "\\r", "quiet": 20, "limit": 300},
               {"dump": "/tmp/end.json"}]}

The screen is emulated with pyte, which also answers the terminal's cursor
and device queries so summonr can measure text. Set SUMMONR_HOME to a
throwaway home with the keys and theme the recording should use.
"""
import codecs, fcntl, json, os, pty, select, struct, sys, termios, time

import pyte

from tui_smoke import pump, send

COLS, ROWS = int(os.environ.get("COLS", "132")), int(os.environ.get("ROWS", "42"))
NAMED = {"black": (0, 0, 0), "red": (205, 49, 49), "green": (13, 188, 121), "yellow": (229, 229, 16), "blue": (36, 114, 200), "magenta": (188, 63, 188), "cyan": (17, 168, 205), "white": (229, 229, 229), "brown": (229, 229, 16)}


def rgb(value, fallback):
    if value == "default":
        return fallback
    if len(value) == 6 and all(c in "0123456789abcdef" for c in value):
        return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4))
    return NAMED.get(value, fallback)


def grid(screen, fg_default):
    cells = []
    for y in range(screen.lines):
        row = screen.buffer[y]
        for x in range(screen.columns):
            c = row[x]
            fg, bg = (c.bg, c.fg) if c.reverse else (c.fg, c.bg)
            cells.append({"ch": c.data, "fg": rgb(fg, fg_default), "bg": None if bg == "default" else rgb(bg, (0, 0, 0)), "bold": c.bold, "italic": c.italics})
    return {"w": screen.columns, "h": screen.lines, "cells": cells}


class Session:
    def __init__(self, plan):
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.out = bytearray()
        self.events = []
        self.start = time.time()
        self.alive = True
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor")
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
            os.chdir(plan["cwd"])
            os.execvpe(plan["argv"][0], plan["argv"], env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        self.screen.write_process_input = lambda data: send(self.fd, data.encode())

    def read(self, seconds):
        before = len(self.out)
        ready, _, _ = select.select([self.fd], [], [], seconds)
        if ready:
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                chunk = b""
            if not chunk:
                self.alive = False
            self.out.extend(chunk)
        fresh = bytes(self.out[before:])
        if fresh:
            text = self.decoder.decode(fresh)
            if text:
                self.events.append([round(time.time() - self.start, 3), "o", text])
            self.stream.feed(fresh)
        return bool(fresh)

    def settle(self, quiet, limit, until_exit):
        end, last = time.time() + limit, time.time()
        while time.time() < end and self.alive:
            if self.read(0.05):
                last = time.time()
            elif not until_exit and time.time() - last > quiet:
                return

    def type(self, text, delay):
        for ch in text:
            send(self.fd, ch.encode())
            end = time.time() + delay
            while time.time() < end:
                self.read(0.01)


def main():
    plan = json.load(open(sys.argv[1]))
    fg = tuple(plan.get("fg", [230, 230, 230]))
    session = Session(plan)
    session.settle(3, 20, False)
    for step in plan["steps"]:
        if "type" in step:
            session.type(step["type"], step.get("delay", 0.03))
        if "keys" in step:
            send(session.fd, step["keys"].encode().decode("unicode_escape").encode())
        session.settle(step.get("quiet", 3), step.get("limit", 30), step.get("until_exit", False))
        if "dump" in step:
            json.dump(grid(session.screen, fg), open(step["dump"], "w"))
            print("dumped", step["dump"], flush=True)
    if session.alive:
        send(session.fd, b"\x03\x03")
        pump(session.fd, session.out, 1)
    if "cast" in plan:
        with open(plan["cast"], "w") as cast:
            cast.write(json.dumps({"version": 2, "width": COLS, "height": ROWS, "timestamp": int(session.start), "env": {"TERM": "xterm-256color"}}) + "\n")
            for event in session.events:
                cast.write(json.dumps(event) + "\n")
        print("cast", plan["cast"], len(session.events), "events", flush=True)


main()
