#!/usr/bin/env python3
"""Drive the summon interface in a pty and assert text on screen.

usage: tui_smoke.py WORKDIR BASE_URL 'line one' 'line two' ... -- needle ...

Each line is typed key by key and sent with Enter; needles are searched in the
captured screen text with escape sequences stripped. Exit 1 if any needle is
missing. Set SUMMONR_HOME to an empty directory first.
"""
import fcntl, os, pty, re, select, struct, sys, termios, time

BIN = os.environ.get("SUMMON", "target/debug/summon")


def main() -> int:
    args = sys.argv[1:]
    split = args.index("--")
    workdir, base_url, lines, needles = args[0], args[1], args[2:split], args[split + 1 :]
    env = dict(
        os.environ,
        SUMMONR_BASE_URL=base_url,
        SUMMONR_MODEL=os.environ.get("SUMMONR_MODEL", "fake"),
        SUMMONR_API_KEY=os.environ.get("SUMMONR_API_KEY", "x"),
        TERM="xterm-256color",
    )
    binary = os.path.abspath(BIN)
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        os.chdir(workdir)
        os.execvpe(binary, ["summon", "--here", "--no-splash"], env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    out = bytearray()

    def pump(seconds: float) -> None:
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                try:
                    out.extend(os.read(fd, 65536))
                except OSError:
                    return

    def send(data: bytes) -> bool:
        try:
            os.write(fd, data)
            return True
        except OSError:
            return False

    alive = True
    pump(4)
    for line in lines:
        for ch in line:
            if not send(ch.encode()):
                alive = False
                break
            pump(0.05)
        if not alive or not send(b"\r"):
            alive = False
            break
        pump(float(os.environ.get("TUI_SMOKE_WAIT", "12")))
    if alive:
        send(b"/exit\r")
        pump(2)
    text = out.decode("utf-8", "replace")
    if not alive:
        _, status = os.waitpid(pid, 0)
        tail = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]", "", text)[-1500:]
        print(f"summon exited early (status {status}); last output:\n{tail}")
    plain = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]", "", text)
    missing = [needle for needle in needles if needle not in plain]
    for needle in needles:
        print(("MISS " if needle in missing else "OK   ") + needle)
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
