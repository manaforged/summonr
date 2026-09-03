import asyncio
import contextlib
import tempfile
import unittest
from pathlib import Path

from harbor.models.agent.context import AgentContext

from summoner_harbor_agent import SummonerAgent


class FakeEnvironment:
    default_user = "root"

    def __init__(self) -> None:
        self.callback = None
        self.downloaded = False

    @contextlib.contextmanager
    def scoped_output_callback(self, callback):
        self.callback = callback
        try:
            yield
        finally:
            self.callback = None

    async def exec(self, **_kwargs):
        await self.callback(
            '{"type":"usage","prompt_tokens":11,"cached_tokens":7,'
            '"completion_tokens":5}\n',
            "stdout",
        )
        raise asyncio.CancelledError

    async def is_dir(self, _path):
        return True

    async def download_dir(self, _source, target):
        Path(target).mkdir(parents=True)
        (Path(target) / "session.jsonl").write_text("saved\n")
        self.downloaded = True


class SummonerAgentTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancelled_run_keeps_streamed_usage_and_session(self):
        with tempfile.TemporaryDirectory() as directory:
            logs = Path(directory)
            agent = SummonerAgent(
                logs_dir=logs,
                model_name="qwen",
                binary_path=logs / "summon",
                base_url="http://localhost:8081/v1",
            )
            context = AgentContext()
            environment = FakeEnvironment()

            with self.assertRaises(asyncio.CancelledError):
                await agent.run("probe", environment, context)

            self.assertEqual(context.n_input_tokens, 11)
            self.assertEqual(context.n_cache_tokens, 7)
            self.assertEqual(context.n_output_tokens, 5)
            self.assertTrue(environment.downloaded)
            self.assertTrue((logs / "sessions" / "session.jsonl").is_file())
            self.assertIn(
                '"type":"usage"', (logs / "summoner.stdout.jsonl").read_text()
            )


if __name__ == "__main__":
    unittest.main()
