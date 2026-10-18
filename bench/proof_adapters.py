import asyncio
import hashlib
import shlex
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

import claude_agent
import grok_plan
import image_user
import omp_agent
import pi_agent
import summonr_agent
from harbor.environments.docker.docker import DockerEnvironment

INSTRUCTION = "- You are given a PyTorch state dictionary."


def sent(arm, **options):
    with tempfile.TemporaryDirectory() as logs:
        agent = arm(logs_dir=Path(logs), model_name="zai/glm-5.3-flash", **options)
        commands = []

        async def record(environment, command, **_):
            commands.append(command)
            return SimpleNamespace(return_code=0, stdout="", stderr="")

        agent.exec_as_agent = record
        try:
            asyncio.run(agent.run(INSTRUCTION, SimpleNamespace(), SimpleNamespace()))
        except Exception:
            pass
        return next(command for command in commands if shlex.quote(INSTRUCTION) in command)


def run_env(arm, env):
    with tempfile.TemporaryDirectory() as logs:
        agent = arm(logs_dir=Path(logs) / "fix-git__a1" / "agent", model_name="anthropic/glm-5.3-flash", extra_env=env)
        seen = []

        async def record(environment, command, env=None, **_):
            seen.append(env or {})
            return SimpleNamespace(return_code=0, stdout="", stderr="")

        agent.exec_as_agent = record
        agent.exec_as_root = record
        try:
            asyncio.run(agent.run(INSTRUCTION, SimpleNamespace(exec=record, upload_file=record, upload_dir=record), SimpleNamespace()))
        except Exception:
            pass
        return next(env for env in seen if "ANTHROPIC_MODEL" in env)


class Adapters(unittest.TestCase):
    def test_claude_code_reaches_the_provider_through_its_own_proxy_label(self):
        env = run_env(claude_agent.Claude, {"ANTHROPIC_API_KEY": "bench-proxy", "BENCH_BASE": "http://host.docker.internal:8095/claude/anthropic", "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "65536"})
        self.assertEqual((env["ANTHROPIC_BASE_URL"], env["ANTHROPIC_MODEL"], env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"]), ("http://host.docker.internal:8095/claude~fix-git__a1/anthropic", "glm-5.3-flash", "65536"))

    def test_an_instruction_that_starts_with_a_dash_is_passed_after_the_end_of_options(self):
        for arm, options in ((pi_agent.Pi, {}), (omp_agent.Omp, {"bun": "1.4.3"})):
            with self.subTest(arm=arm.__name__):
                self.assertIn(f" -- {shlex.quote(INSTRUCTION)} ", sent(arm, **options))



def login_commands(arm, user, **options):
    with tempfile.TemporaryDirectory() as logs:
        agent = arm(logs_dir=Path(logs) / "risk-scorer-replay__a1" / "agent", **options)
        sent = []

        async def record(environment, command, env=None, **_):
            sent.append((command, env or {}))
            return SimpleNamespace(return_code=0, stdout="lent\n" if "cp -a" in command else "", stderr="")

        agent.exec_as_agent = record
        agent.exec_as_root = record
        try:
            asyncio.run(agent.run(INSTRUCTION, SimpleNamespace(default_user=user), SimpleNamespace()))
        except Exception:
            pass
        return sent


class ImageUser(unittest.TestCase):
    def environment(self, found):
        environment = object.__new__(image_user.Docker)
        environment.default_user = None

        async def run(command, user=None, **_):
            return SimpleNamespace(return_code=0, stdout=found, stderr="")

        async def started(self, force_build):
            return None

        environment.exec = run
        original = DockerEnvironment.start
        DockerEnvironment.start = started
        try:
            asyncio.run(environment.start(False))
        finally:
            DockerEnvironment.start = original
        return environment

    def test_a_task_without_an_agent_user_runs_agents_as_the_image_user(self):
        environment = self.environment("nobody\n")
        with environment.with_default_user(None):
            self.assertEqual(environment.default_user, "nobody")
        with environment.with_default_user("root"):
            self.assertEqual(environment.default_user, "root")
        self.assertIsNone(environment.default_user)
        root = self.environment("")
        with root.with_default_user(None):
            self.assertIsNone(root.default_user)

    def test_agents_read_a_private_copy_of_logins_the_image_user_cannot_read(self):
        with tempfile.NamedTemporaryFile() as binary:
            pin = hashlib.sha256(b"").hexdigest()
            arms = (
                (summonr_agent.Summonr, "summonr", {"model_name": "openai/gpt-6.1-sol", "binary": binary.name, "binary_sha256": pin}),
                (grok_plan.GrokPlan, "grok", {"model_name": "xai/grok-4.7"}),
            )
            for arm, name, options in arms:
                with self.subTest(arm=arm.__name__):
                    lent = login_commands(arm, "nobody", **options)
                    commands = " ".join(command for command, _ in lent)
                    self.assertIn(f"cp -a /opt/auth/{name} /opt/lent/{name} ", commands)
                    self.assertIn(f"chown -R nobody /opt/lent/{name}", commands)
                    self.assertIn(f"d=/opt/auth/{name}/$n", commands)
                    self.assertIn('! cmp -s "$d" "$o" || cat "$f" > "$d"', commands)
                    self.assertIn(f"rm -rf /opt/lent/{name} /opt/lent/{name}.orig", commands)
                    reads = {"summonr": "ln -sfn /opt/lent/summonr/credentials.toml ", "grok": "/opt/lent/grok/auth.json"}[name]
                    self.assertTrue(any(reads in command or reads in env.values() for command, env in lent), lent)
                    root = login_commands(arm, None, **options)
                    self.assertNotIn("/opt/lent", " ".join(command for command, _ in root) + " ".join(str(env) for _, env in root))
                    owned = {"summonr": "ln -sfn /opt/auth/summonr/credentials.toml ", "grok": "/opt/auth/grok/auth.json"}[name]
                    self.assertTrue(any(owned in command or owned in env.values() for command, env in root), root)

if __name__ == "__main__":
    unittest.main()
