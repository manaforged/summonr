import asyncio
import shlex
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

import claude_agent
import omp_agent
import pi_agent

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


if __name__ == "__main__":
    unittest.main()
