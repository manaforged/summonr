import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path

from proof_measure import HERE, free_port, wait_port


def proxy_pids(run):
    return subprocess.run(["pgrep", "-f", f"^python3 .*proxy.py .*--log [^ ]*/{run}\\.jsonl"], capture_output=True, text=True).stdout.split()


class Proxy(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.procs = []

    def tearDown(self):
        for proc in self.procs:
            proc.kill()
            proc.wait()

    def proxy(self, port, upstream, log, *extra):
        proc = subprocess.Popen([sys.executable, str(HERE / "proxy.py"), "--port", str(port), "--upstream", upstream, "--log", str(log), *extra], stderr=subprocess.DEVNULL)
        self.procs.append(proc)
        wait_port(port)
        return proc

    def test_an_upstream_that_hangs_up_is_logged(self):
        upstream = socket.socket()
        upstream.bind(("127.0.0.1", 0))
        upstream.listen()
        def hang_up():
            conn, _ = upstream.accept()
            conn.recv(65536)
            conn.close()
        threading.Thread(target=hang_up, daemon=True).start()
        port, log = free_port(), self.tmp / "run.jsonl"
        self.proxy(port, f"http://127.0.0.1:{upstream.getsockname()[1]}", log)
        body = json.dumps({"model": "glm-5.3-flash", "messages": [{"role": "user", "content": "hi"}]}).encode()
        request = urllib.request.Request(f"http://127.0.0.1:{port}/pi~fix-git__a1/v1/chat/completions", data=body, headers={"Content-Type": "application/json"})
        with self.assertRaises(urllib.error.HTTPError) as caught:
            urllib.request.urlopen(request, timeout=10)
        caught.exception.close()
        upstream.close()
        self.assertEqual(caught.exception.code, 502)
        [record] = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual((record["label"], record["status"]), ("fix-git__a1", 502))

    def test_an_upstream_that_never_finishes_the_handshake_fails_fast(self):
        upstream = socket.socket()
        upstream.bind(("127.0.0.1", 0))
        upstream.listen()
        port, log = free_port(), self.tmp / "run.jsonl"
        self.proxy(port, f"https://127.0.0.1:{upstream.getsockname()[1]}", log, "--connect-timeout", "1")
        body = json.dumps({"model": "glm-5.3-flash", "messages": [{"role": "user", "content": "hi"}]}).encode()
        request = urllib.request.Request(f"http://127.0.0.1:{port}/pi~fix-git__a1/v1/chat/completions", data=body, headers={"Content-Type": "application/json"})
        started = time.monotonic()
        with self.assertRaises(urllib.error.HTTPError) as caught:
            urllib.request.urlopen(request, timeout=20)
        caught.exception.close()
        upstream.close()
        self.assertEqual(caught.exception.code, 502)
        self.assertLess(time.monotonic() - started, 10)

    def test_stopping_a_run_ends_its_harbor_and_proxy_and_leaves_other_runs(self):
        run, other = f"stop-{os.getpid()}-a", f"stop-{os.getpid()}-b"
        (self.tmp / "logs").mkdir()
        mine = self.proxy(free_port(), "http://127.0.0.1:9", self.tmp / "logs" / f"{run}.jsonl")
        theirs = self.proxy(free_port(), "http://127.0.0.1:9", self.tmp / "logs" / f"{other}.jsonl")
        harbor = subprocess.Popen(["bash", "-c", f'exec -a "harbor job resume -p {self.tmp}/jobs/{run}" sleep 60'])
        self.procs.append(harbor)
        stopped = subprocess.run([str(HERE / "board.sh"), "stop", run], capture_output=True, text=True, timeout=120)
        self.assertEqual((stopped.returncode, harbor.wait(5), mine.wait(5) is not None, theirs.poll()), (0, -2, True, None), stopped.stderr)

    def test_every_proxied_pair_preflights_a_task_of_its_dataset(self):
        tasks = (HERE / "tasks-tb21.txt").read_text().split()
        for proxy in sorted((HERE / "pairs").glob("*.proxy")):
            pins = dict(line.split(None, 1) for line in proxy.read_text().splitlines() if line.strip())
            self.assertIn("terminal-bench-2-1", proxy.with_suffix(".yaml").read_text(), proxy.name)
            self.assertIn(pins.get("preflight", ""), tasks, proxy.name)

    def test_starting_a_run_never_kills_another_runs_proxy(self):
        port = free_port()
        (self.tmp / "logs").mkdir()
        other = self.proxy(port, "http://127.0.0.1:9", self.tmp / "logs" / "first.jsonl")
        script = f'root={self.tmp}; . {HERE}/proxy-start.sh; start_proxy second {port} http://127.0.0.1:9 "" ""'
        started = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=60)
        self.assertEqual((started.returncode, other.poll()), (1, None))
        self.assertIn(f"port {port} is held by another run", started.stderr)

    def test_a_proxy_that_dies_comes_back_until_its_run_stops(self):
        port, run = free_port(), f"sup-{os.getpid()}"
        (self.tmp / "logs").mkdir()
        (self.tmp / "proxy.py").symlink_to(HERE / "proxy.py")
        shell = lambda line: subprocess.run(["bash", "-c", f"root={self.tmp}; . {HERE}/proxy-start.sh; {line}"], check=True, timeout=60)
        shell(f'start_proxy {run} {port} http://127.0.0.1:9 "" ""')
        first = proxy_pids(run)
        subprocess.run(["kill", *first], check=True)
        for _ in range(100):
            if (now := proxy_pids(run)) and now != first:
                break
            time.sleep(0.1)
        wait_port(port)
        shell(f"stop_proxy {run}")
        time.sleep(2)
        self.assertEqual((now != first, proxy_pids(run)), (True, []))
        wait_port(port, up=False)


class Resume(unittest.TestCase):
    def test_a_resume_starts_harbor_without_asking_the_hub_again(self):
        tmp = Path(tempfile.mkdtemp())
        bench = tmp / "bench"
        shutil.copytree(HERE, bench, ignore=shutil.ignore_patterns("jobs", "logs", "tools", "tools-*", "datasets", "results", "__pycache__"))
        for args in (["init", "-q"], ["add", "-A"], ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "t"]):
            subprocess.run(["git", "-C", str(bench), *args], check=True, capture_output=True)
        pin = dict(line.split() for line in (HERE / "tools.lock").read_text().splitlines() if line.strip())["harbor"]
        fake = tmp / "bin"
        fake.mkdir()
        (fake / "harbor").write_text(f'#!/bin/sh\ncase "$1" in --version) echo {pin} ;; download) exit 1 ;; job) touch {tmp}/resumed ;; esac\n')
        (fake / "harbor").chmod(0o755)
        (bench / "jobs" / "run1").mkdir(parents=True)
        (bench / "jobs" / "run1" / "run.txt").write_text("bench x\n")
        done = subprocess.run([str(bench / "board.sh"), "run1", "chatgpt"], env={**os.environ, "PATH": f"{fake}:{os.environ['PATH']}", "BENCH_SUMMONR": "/bin/sh"}, capture_output=True, text=True, timeout=120)
        self.assertEqual((done.returncode, (tmp / "resumed").exists()), (0, True), done.stderr)


if __name__ == "__main__":
    unittest.main()
