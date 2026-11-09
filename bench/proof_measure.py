import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import audit
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
        "verifier_result": {"rewards": {"reward": reward}} if reward is not None else None,
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

    def test_a_run_resumed_under_another_bench_commit_is_refused(self):
        (self.job / "run.txt").write_text("bench aaaa\nresume 2026-10-23T00:00:00Z bench aaaa\nresume 2026-10-23T01:00:00Z bench bbbb\n")
        self.assertEqual(self.refusals([]), ["run1 resumed under bench bbbb, started under aaaa"])


class Reruns(Bench):
    def test_only_infrastructure_failures_and_silent_crashes_rerun(self):
        harbor_trial(self.job, "fix-git__a1", "fix-git", reward=None, exception="AgentSetupTimeoutError")
        harbor_trial(self.job, "fix-git__a2", "fix-git", reward=0.0, exception="NonZeroAgentExitCodeError")
        harbor_trial(self.job, "fix-git__a3", "fix-git", reward=0.0, exception="NonZeroAgentExitCodeError")
        harbor_trial(self.job, "fix-git__a4", "fix-git", reward=0.0, exception="AgentTimeoutError")
        harbor_trial(self.job, "fix-git__a5", "fix-git", reward=0.0)
        harbor_trial(self.job, "fix-git__a6", "fix-git", reward=0.0, exception="ApiRateLimitError")
        by_label = {"fix-git__a3": [call("fix-git__a3")], "fix-git__a4": [call("fix-git__a4")], "fix-git__a6": [call("fix-git__a6"), {**call("fix-git__a6"), "status": 429}]}
        self.assertEqual(rerun.eligible(self.job, by_label), [("fix-git__a1", "AgentSetupTimeoutError"), ("fix-git__a2", "NonZeroAgentExitCodeError"), ("fix-git__a6", "ApiRateLimitError")])

    def test_a_provider_failure_reruns_only_when_the_proxy_saw_it(self):
        harbor_trial(self.job, "fix-git__b1", "fix-git", reward=0.0, exception="ApiRateLimitError")
        harbor_trial(self.job, "fix-git__b2", "fix-git", reward=0.0, exception="NetworkConnectionError")
        harbor_trial(self.job, "fix-git__b3", "fix-git", reward=0.0, exception="NetworkConnectionError")
        by_label = {"fix-git__b1": [call("fix-git__b1")], "fix-git__b2": [call("fix-git__b2")], "fix-git__b3": [call("fix-git__b3"), {**call("fix-git__b3"), "status": 502}]}
        self.assertEqual(rerun.eligible(self.job, by_label), [("fix-git__b3", "NetworkConnectionError")])

    def test_a_trial_the_provider_cut_reruns_for_every_arm_and_a_solve_or_timeout_stands(self):
        harbor_trial(self.job, "fix-git__p1", "fix-git", arm="pi", reward=0.0)
        harbor_trial(self.job, "fix-git__p2", "fix-git", arm="opencode2", reward=0.0, exception="NonZeroAgentExitCodeError")
        harbor_trial(self.job, "fix-git__p3", "fix-git", arm="summonr", exception="ApiRateLimitError")
        harbor_trial(self.job, "fix-git__p4", "fix-git", arm="opencode2", reward=0.0, exception="AgentTimeoutError")
        harbor_trial(self.job, "fix-git__p5", "fix-git", arm="pi", reward=0.0)
        cut = lambda name: [call(name), {**call(name), "status": 429}]
        by_label = {"fix-git__p1": cut("fix-git__p1"), "fix-git__p2": cut("fix-git__p2"), "fix-git__p3": cut("fix-git__p3"), "fix-git__p5": [{**call("fix-git__p5"), "status": 429}, call("fix-git__p5")]}
        self.assertEqual(rerun.eligible(self.job, by_label), [("fix-git__p1", "-"), ("fix-git__p2", "NonZeroAgentExitCodeError")])

    def test_first_request_tokens_come_from_the_first_request_with_tools(self):
        fields = trial.token_fields([call("t", prompt=513, tools=0), call("t", prompt=9000), call("t", prompt=9500)], None)
        self.assertEqual((fields["first_prompt"], fields["session_prompt"]), (9000, 10013))

    def test_a_resume_sets_aside_unfinished_and_verifier_cut_trials(self):
        harbor_trial(self.job, "fix-git__c1", "fix-git")
        harbor_trial(self.job, "fix-git__c2", "fix-git", reward=None)
        harbor_trial(self.job, "fix-git__c3", "fix-git", reward=None, exception="AgentTimeoutError")
        (self.job / "fix-git__c4" / "agent").mkdir(parents=True)
        harbor_trial(self.job, "fix-git__c5", "fix-git")
        (self.job / "fix-git__c5" / "result.json").write_text("")
        harbor_trial(self.job, "fix-git__c6", "fix-git", reward=None, exception="AgentSetupTimeoutError")
        harbor_trial(self.job, "fix-git__c7", "fix-git", reward=None, exception="CancelledError")
        calls = {name: [call(name)] for name in ("fix-git__c1", "fix-git__c2", "fix-git__c3", "fix-git__c7")}
        rerun.set_aside(self.job, calls, rerun=True)
        self.assertEqual(sorted(path.name for path in (self.jobs / "run1.rerun").iterdir()), ["fix-git__c2", "fix-git__c4", "fix-git__c5", "fix-git__c6", "fix-git__c7"])
        self.assertEqual((self.job / "run.txt").read_text(), "dropped fix-git__c2\ndropped fix-git__c4\ndropped fix-git__c5\ndropped fix-git__c7\nrerun fix-git__c6 AgentSetupTimeoutError\n")
        self.assertEqual(sorted(path.name for path in self.job.iterdir() if path.is_dir()), ["fix-git__c1", "fix-git__c3"])

    def test_listing_the_trials_a_resume_would_move_moves_nothing(self):
        harbor_trial(self.job, "fix-git__d1", "fix-git")
        (self.job / "fix-git__d2" / "agent").mkdir(parents=True)
        (self.job / "run.txt").write_text("bench aaaa\n")
        listed = subprocess.run([sys.executable, str(HERE / "rerun.py"), str(self.job), str(self.tmp / "logs" / "run1.jsonl"), "--rerun"], capture_output=True, text=True, check=True).stdout
        self.assertEqual((listed, sorted(path.name for path in self.job.iterdir() if path.is_dir()), (self.jobs / "run1.rerun").exists(), (self.job / "run.txt").read_text()), ("dropped fix-git__d2\n", ["fix-git__d1", "fix-git__d2"], False, "bench aaaa\n"))

    def test_pairs_retry_a_failed_setup_and_never_an_agent_failure(self):
        import yaml
        from harbor.models.job.config import JobConfig
        from harbor.trial.queue import TrialQueue

        import level

        for pair in sorted((HERE / "pairs").glob("*.yaml")):
            config = JobConfig.model_validate(yaml.safe_load(pair.read_text()))
            queue = TrialQueue(n_concurrent=1, retry_config=config.retry)
            retried = [kind for kind in (level.BaseInstallError.__name__, level.HarnessInstallError.__name__, "AgentSetupTimeoutError", "NonZeroAgentExitCodeError", "AgentTimeoutError", "ApiRateLimitError") if queue._should_retry_exception(kind)]
            self.assertEqual((retried, config.retry.max_retries > 0), (["BaseInstallError", "HarnessInstallError", "AgentSetupTimeoutError"], True), pair.name)

    def test_pairs_pin_the_dataset_as_a_ref_harbor_reads(self):
        for pair in sorted((HERE / "pairs").glob("*.yaml")):
            datasets = pair.read_text().split("\ndatasets:\n", 1)[1]
            self.assertNotIn("version:", datasets, pair.name)
            self.assertIn("ref:", datasets, pair.name)

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
