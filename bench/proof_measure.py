import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import asyncio
import unittest
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import audit
import level
import rerun
import trial


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_port(port, up=True):
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
            if up:
                return
        except OSError:
            if not up:
                return
        time.sleep(0.05)
    raise AssertionError(f"port {port} never became {'open' if up else 'closed'}")


def harbor_trial(job, name, task, arm="pi", reward=1.0, ran=True, snapshot=None, checksum="c1", exception=None):
    folder = job / name
    (folder / "agent").mkdir(parents=True)
    (folder / "config.json").write_text(json.dumps({"agent": {"name": None, "import_path": f"{arm}_agent:Arm"}}))
    (folder / "result.json").write_text(json.dumps({
        "agent_info": {"name": arm, "version": "1", "model_info": {"name": "glm-5.3-flash"}},
        "task_name": f"terminal-bench/{task}",
        "trial_name": name,
        "task_checksum": checksum,
        "config": {"agent": {"kwargs": {"reasoning_effort": "high"}}},
        "verifier_result": {"rewards": {"reward": reward}},
        "agent_result": {"n_input_tokens": 5000, "n_output_tokens": 100},
        "agent_execution": {"started_at": "2026-10-15T00:00:00Z", "finished_at": "2026-10-15T00:01:00Z"} if ran else None,
        "exception_info": {"exception_type": exception} if exception else None,
    }))
    if snapshot is not None:
        (folder / "agent" / "parity.txt").write_text(snapshot)


def call(label, prompt=1000, model="glm-5.3-flash", effort="high", thinking=None, tools=6):
    return {
        "label": label, "status": 200, "response_model": model, "started": 0.0, "ended": 1.0,
        "usage": {"prompt_tokens": prompt, "completion_tokens": 10},
        "request": {"reasoning_effort": effort, "thinking": thinking, "max_tokens": 131072, "temperature": None, "stream": True, "tools": tools},
    }


class Bench(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.jobs = self.tmp / "jobs"
        self.job = self.jobs / "run1"
        self.job.mkdir(parents=True)
        (self.job / "job.yaml").write_text("n_attempts: 1\n")
        (self.tmp / "logs").mkdir()

    def log(self, calls):
        with open(self.tmp / "logs" / "run1.jsonl", "w") as out:
            out.writelines(json.dumps(row) + "\n" for row in calls)


class TokenRows(Bench):
    def test_a_proxied_trial_with_no_calls_counts_zero_not_its_own_report(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        [row] = trial.trials(self.jobs, "run1", {}, None)
        self.assertEqual((row["prompt"], row["requests"], row["reported_prompt"]), (0, 0, 5000))

    def test_the_model_and_effort_come_from_the_traffic(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        [row] = trial.trials(self.jobs, "run1", {"fix-git__a1": [call("fix-git__a1", model="glm-4.5-air", effort=None, thinking={"type": "enabled"})]}, None)
        self.assertEqual((row["models"], row["effort"], row["prompt"]), (["glm-4.5-air"], ["None/enabled"], 1000))


class Refusals(Bench):
    def refusals(self, rows, pinned=False):
        return audit.refusals(self.jobs, ["run1"], rows, "nosuchpair", pinned)

    def test_a_proxy_label_with_no_trial_is_refused(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", snapshot="env")
        self.log([call("fix-git__a1"), call("pi"), call("pi")])
        self.assertEqual(self.refusals([]), ["proxy label pi matches no trial in run1 (2 requests)"])

    def test_a_pinned_run_without_a_settings_file_is_refused(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", snapshot="env")
        self.assertEqual(self.refusals([], pinned=True), ["nosuchpair.settings is missing: run the preflight and commit the settings it writes"])

    def test_a_task_that_changed_during_the_run_is_refused(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", checksum="c1")
        harbor_trial(self.job, "fix-git__a2", "fix-git", checksum="c2")
        rows = trial.trials(self.jobs, "run1", {}, None)
        self.assertEqual(self.refusals(rows), ["fix-git: 2 task checksums in one run"])


class Reruns(Bench):
    def test_only_infrastructure_failures_and_silent_crashes_rerun(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", exception="AgentSetupTimeoutError")
        harbor_trial(self.job, "fix-git__a2", "fix-git", exception="NonZeroAgentExitCodeError")
        harbor_trial(self.job, "fix-git__a3", "fix-git", exception="NonZeroAgentExitCodeError")
        harbor_trial(self.job, "fix-git__a4", "fix-git", exception="AgentTimeoutError")
        harbor_trial(self.job, "fix-git__a5", "fix-git", reward=0.0)
        harbor_trial(self.job, "fix-git__a6", "fix-git", exception="ApiRateLimitError")
        by_label = {"fix-git__a3": [call("fix-git__a3")], "fix-git__a4": [call("fix-git__a4")], "fix-git__a6": [call("fix-git__a6"), {**call("fix-git__a6"), "status": 429}]}
        self.assertEqual(rerun.eligible(self.job, by_label), [("fix-git__a1", "AgentSetupTimeoutError"), ("fix-git__a2", "NonZeroAgentExitCodeError"), ("fix-git__a6", "ApiRateLimitError")])

    def test_a_provider_failure_reruns_only_when_the_proxy_saw_it(self):
        harbor_trial(self.job, "fix-git__b1", "fix-git", exception="ApiRateLimitError")
        harbor_trial(self.job, "fix-git__b2", "fix-git", exception="NetworkConnectionError")
        harbor_trial(self.job, "fix-git__b3", "fix-git", exception="NetworkConnectionError")
        by_label = {"fix-git__b1": [call("fix-git__b1")], "fix-git__b2": [call("fix-git__b2")], "fix-git__b3": [call("fix-git__b3"), {**call("fix-git__b3"), "status": 502}]}
        self.assertEqual(rerun.eligible(self.job, by_label), [("fix-git__b3", "NetworkConnectionError")])

    def test_calls_from_a_dropped_or_rerun_trial_are_not_orphans(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        (self.job / "run.txt").write_text("model glm-5.3-flash\ndropped fix-git__gone\nrerun fix-git__old AgentSetupTimeoutError\n")
        self.log([call("fix-git__gone"), call("fix-git__old"), call("fix-git__stray")])
        self.assertEqual(audit.orphans(self.job, audit.requests([self.tmp / "logs" / "run1.jsonl"])), ["proxy label fix-git__stray matches no trial in run1 (1 requests)"])


class Shells(Bench):
    def test_a_runtime_only_one_arm_reaches_fails_the_probe(self):
        for name, arm, extra in (("env-probe__a1", "pi", ""), ("env-probe__a2", "hermes", "run uv /root/.local/bin/uv uv 0.9.5\n")):
            harbor_trial(self.job, name, "env-probe", arm=arm)
            (self.job / name / "verifier").mkdir()
            (self.job / name / "verifier" / "env.txt").write_text("path /usr/bin\nrun python3 /usr/local/bin/python3 Python 3.13.9\n" + extra)
        seen, differ = audit.shells(self.job)
        self.assertEqual((sorted(seen), differ), (["hermes", "pi"], ["hermes shell reaches ['run uv /root/.local/bin/uv uv 0.9.5']"]))

    def test_replies_cut_at_the_length_limit_are_counted(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        rows = trial.trials(self.jobs, "run1", {}, None)
        capped = {**call("fix-git__a1"), "finish_reason": "length"}
        line = audit.proxy_check(rows, {"fix-git__a1": [capped, call("fix-git__a1")]}).splitlines()[-1]
        self.assertEqual([cell.strip() for cell in line.strip("|").split("|")][6:8], ["0", "1"])


class Scan(Bench):
    def test_a_harness_database_is_not_scanned_but_its_logs_are(self):
        agent = self.tmp / "agent"
        (agent / "opencode").mkdir(parents=True)
        catalog = b'{"doc":"https://requesty.ai/solution/llm-routing","note":"see https://www.tbench.ai/leaderboard"}'
        (agent / "opencode" / "opencode.db").write_bytes(b"SQLite format 3\0" + catalog)
        (agent / "opencode.txt").write_text('{"tool":"shell","input":{"command":"git log"}}')
        self.assertEqual(trial.suspect(agent), ([], []))
        (agent / "opencode.txt").write_text('{"tool":"shell","input":{"command":"curl https://www.tbench.ai/tasks/fix-git"}}')
        self.assertEqual(trial.suspect(agent), (["https://www.tbench"], ["tbench.ai"]))


class Preflight(Bench):
    def test_tool_requests_below_the_protocol_effort_or_from_another_model_fail(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", arm="hermes")
        rows = trial.trials(self.jobs, "run1", {}, None)
        tools = lambda effort, model="glm-5.3-flash": {**call("fix-git__a1", model=model, effort=effort), "request": {**call("fix-git__a1", effort=effort)["request"], "tools": 18}}
        by_label = {"fix-git__a1": [tools("high"), tools(None), tools(None), call("fix-git__a1", effort="low", tools=0), tools("high", model="glm-4.5-flash")]}
        self.assertEqual(audit.unpinned(rows, by_label, "high", "glm-5.3-flash"), ["hermes: 1 responses came from glm-4.5-flash", "hermes: 2 tool requests sent effort None, not high"])


class Startup(Bench):
    def test_an_installers_shell_startup_edits_are_gone_before_the_model_runs(self):
        home = self.tmp / "root"
        home.mkdir()
        (home / ".bashrc").write_text("alias ll='ls -l'\n")
        level.TASK_PATH, level.RC_DIR, level.HOMES = str(self.tmp / "task-path"), str(self.tmp / "rc"), str(home)
        env = {"HOME": str(home), "PATH": "/usr/bin:/bin"}
        subprocess.run(["bash", "-c", level.keep()], env=env, check=True)
        (home / ".bashrc").write_text("alias ll='ls -l'\nexport PATH=\"$HOME/.bun/bin:$PATH\"\n")
        (home / ".profile").write_text(". ~/.bashrc\n")
        subprocess.run(["bash", "-c", level.restore()], env=env, check=True)
        self.assertEqual(((home / ".bashrc").read_text(), (home / ".profile").exists()), ("alias ll='ls -l'\n", False))


class Install(unittest.TestCase):
    class Agent:
        def __init__(self, fails):
            self.fails, self.sent = fails, []

        async def ensure_system_dependencies(self, environment, packages):
            self.sent.append(("packages", packages))
            if self.fails:
                raise RuntimeError("E: Failed to fetch curl_7.74.0-1.3+deb11u16_amd64.deb 404 Not Found")

        async def exec_as_root(self, environment, command):
            self.sent.append(("root", command))

    def test_a_mirror_that_lost_its_packages_falls_back_to_the_images_pinned_snapshot(self):
        agent = self.Agent(fails=True)
        asyncio.run(level.install(agent, None))
        (_, packages), (kind, command) = agent.sent
        self.assertEqual((packages, kind), (level.PACKAGES, "root"))
        self.assertIn("snapshot.debian.org", command)
        self.assertTrue(command.endswith("install -y -qq ca-certificates curl git ripgrep xz-utils unzip bash coreutils python3 nodejs npm"))

    def test_a_working_mirror_installs_once(self):
        agent = self.Agent(fails=False)
        asyncio.run(level.install(agent, None))
        self.assertEqual(agent.sent, [("packages", level.PACKAGES)])


class Parity(Bench):
    def test_a_second_attempt_that_saw_another_environment_is_flagged(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", snapshot="PATH=/usr/bin\n")
        harbor_trial(self.job, "fix-git__a2", "fix-git", snapshot="PATH=/usr/bin:/root/.bun/bin\n")
        self.assertEqual(audit.parity(self.job), ["fix-git: 2 distinct environments across ['pi_agent:Arm']"])

    def test_an_agent_that_ran_without_a_snapshot_is_flagged(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", snapshot="env")
        harbor_trial(self.job, "fix-git__a2", "fix-git")
        harbor_trial(self.job, "fix-git__a3", "fix-git", ran=False)
        self.assertEqual(audit.parity(self.job), ["fix-git__a2: the agent ran with no environment snapshot"])


class Settings(Bench):
    def test_effort_and_thinking_are_recorded_together(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        rows = trial.trials(self.jobs, "run1", {}, None)
        by_label = {"fix-git__a1": [call("fix-git__a1", thinking={"type": "enabled"}), call("fix-git__a1", thinking={"type": "disabled"})]}
        self.assertEqual(audit.observed(rows, by_label), {"pi": {("high/enabled", "131072", "None"), ("high/disabled", "131072", "None")}})

    def test_every_setting_line_the_preflight_saw_is_allowed(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        rows = trial.trials(self.jobs, "run1", {}, None)
        settings = self.tmp / "x.settings"
        settings.write_text("# arm effort max_tokens temperature\npi high/enabled 131072 None\npi None/None 131072 None\n")
        by_label = {"fix-git__a1": [call("fix-git__a1", thinking={"type": "enabled"}), call("fix-git__a1", effort=None)]}
        self.assertEqual(audit.drift(rows, by_label, audit.expectations(settings)), [])
        by_label["fix-git__a1"].append(call("fix-git__a1", effort="low", thinking={"type": "enabled"}))
        self.assertEqual(audit.drift(rows, by_label, audit.expectations(settings)), ["pi: settings file names [('None/None', '131072', 'None'), ('high/enabled', '131072', 'None')], requests sent [('low/enabled', '131072', 'None')]"])

    def test_the_settings_file_pins_tool_requests_and_the_table_lists_the_rest(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git")
        rows = trial.trials(self.jobs, "run1", {}, None)
        settings = self.tmp / "x.settings"
        settings.write_text("# arm effort max_tokens temperature\npi high/enabled 131072 None\n")
        helper = call("fix-git__a1", effort="low", tools=0)
        helper["request"].update(max_tokens=1024, temperature=0)
        by_label = {"fix-git__a1": [call("fix-git__a1", thinking={"type": "enabled"}), helper]}
        self.assertEqual(audit.drift(rows, by_label, audit.expectations(settings)), [])
        self.assertTrue(audit.proxy_check(rows, by_label).splitlines()[-1].endswith("| high/enabled,low/None | 1024,131072 | 0,None |"))


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

    def test_starting_a_run_never_kills_another_runs_proxy(self):
        port = free_port()
        (self.tmp / "logs").mkdir()
        other = self.proxy(port, "http://127.0.0.1:9", self.tmp / "logs" / "first.jsonl")
        script = f'root={self.tmp}; . {HERE}/proxy-start.sh; start_proxy second {port} http://127.0.0.1:9 "" ""'
        started = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=60)
        self.assertEqual((started.returncode, other.poll()), (1, None))
        self.assertIn(f"port {port} is held by another run", started.stderr)



class LongContext(unittest.TestCase):
    rates = {"hit": 0.50, "miss": 2.00, "output": 6.00, "long": {"at": 200_000, "hit": 1.00, "miss": 4.00, "output": 12.00}}

    def call(self, prompt, cached, output):
        return {"usage": {"prompt_tokens": prompt, "completion_tokens": output, "prompt_tokens_details": {"cached_tokens": cached}}}

    def test_a_request_at_the_threshold_is_billed_at_the_long_rate(self):
        calls = [self.call(100_000, 90_000, 1_000), self.call(250_000, 240_000, 1_000)]
        short = (90_000 * 0.50 + 10_000 * 2.00 + 1_000 * 6.00) / 1_000_000
        long = (240_000 * 1.00 + 10_000 * 4.00 + 1_000 * 12.00) / 1_000_000
        self.assertAlmostEqual(trial.token_fields(calls, self.rates)["cost"], short + long)

    def test_totals_without_request_sizes_are_not_priced_under_a_tiered_rate(self):
        context = {"n_input_tokens": 900_000, "n_cache_tokens": 800_000, "n_output_tokens": 30_000}
        self.assertIsNone(trial.standard_tokens(context, {}, self.rates)["cost"])



class ProxyArg(unittest.TestCase):
    def test_a_named_proxy_log_is_read_on_a_standard_job(self):
        import table
        with tempfile.TemporaryDirectory() as tmp:
            jobs = Path(tmp) / "jobs"
            (jobs / "run" / "t1").mkdir(parents=True)
            (jobs / "run" / "job.yaml").write_text("{}")
            log = Path(tmp) / "proxy.jsonl"
            log.write_text("")
            args = table.argparse.Namespace(runs=["run"], jobs=str(jobs), proxy=[str(log)])
            table.gather(args, None)


if __name__ == "__main__":
    unittest.main()
