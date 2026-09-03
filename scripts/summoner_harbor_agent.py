import json
import shlex
from pathlib import Path
from typing import Any

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext


class SummonerAgent(BaseAgent):
    def __init__(
        self,
        *args: Any,
        binary_path: str,
        base_url: str,
        max_turns: int = 2,
        max_rounds: int = 32,
        max_secs: int | None = None,
        idle_timeout_secs: int = 600,
        effort: str = "medium",
        revision: str = "workspace",
        **kwargs: Any,
    ) -> None:
        super().__init__(*args, **kwargs)
        self.binary_path = Path(binary_path)
        self.base_url = base_url.rstrip("/")
        self.max_turns = max_turns
        self.max_rounds = max_rounds
        self.max_secs = max_secs
        self.idle_timeout_secs = idle_timeout_secs
        self.effort = effort
        self.revision = revision

    @staticmethod
    def name() -> str:
        return "summoner"

    def version(self) -> str:
        return f"0.1.0+{self.revision}"

    async def setup(self, environment: BaseEnvironment) -> None:
        if not self.binary_path.is_file():
            raise FileNotFoundError(self.binary_path)
        if not self.model_name:
            raise ValueError("model_name is required")
        for name, value in (
            ("max_turns", self.max_turns),
            ("max_rounds", self.max_rounds),
            ("idle_timeout_secs", self.idle_timeout_secs),
        ):
            if value < 1:
                raise ValueError(f"{name} must be at least 1")
        if self.max_secs is not None and self.max_secs < 1:
            raise ValueError("max_secs must be at least 1")

        remote_binary = "/usr/local/bin/summon"
        remote_home = "/tmp/summoner-home"
        remote_config = f"{remote_home}/.mage/config.toml"
        await environment.upload_file(self.binary_path, remote_binary)
        await environment.exec(
            command=f"chmod 0755 {remote_binary} && mkdir -p {remote_home}/.mage",
            user="root",
        )

        config = self.logs_dir / "summoner-config.toml"
        config.write_text(
            'default = "bench"\n'
            'secrets = "file"\n\n'
            "[endpoint.bench]\n"
            f"base_url = {self.base_url!r}\n"
            f"model = {self.model_name!r}\n"
            "context_window = 196608\n"
            f"idle_timeout_secs = {self.idle_timeout_secs}\n"
            f"effort = {self.effort!r}\n\n"
            "[endpoint.bench.body]\n"
            "temperature = 1.0\n"
            "top_p = 0.95\n"
            "top_k = 20\n"
            "min_p = 0\n"
            "presence_penalty = 0\n"
            "repetition_penalty = 1\n"
        )
        await environment.upload_file(config, remote_config)
        owner = environment.default_user
        ownership = (
            f"chown -R {shlex.quote(str(owner))} {remote_home} && "
            if owner is not None
            else ""
        )
        await environment.exec(
            command=f"{ownership}chmod 0600 {remote_config}",
            user="root",
        )

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        command = [
            "/usr/local/bin/summon",
            "bench",
            "--no-splash",
            "--json",
            "--here",
            "--no-sandbox",
            "--net",
            "--no-verify",
            "--effort",
            self.effort,
            "--max-turns",
            str(self.max_turns),
            "--max-rounds",
            str(self.max_rounds),
        ]
        if self.max_secs is not None:
            command.extend(("--max-secs", str(self.max_secs)))
        command.extend(("--goal", instruction))

        stdout = self.logs_dir / "summoner.stdout.log"
        stderr = self.logs_dir / "summoner.stderr.txt"
        stdout.write_text("")
        stderr.write_text("")

        async def capture(text: str, stream: str) -> None:
            path = stderr if stream == "stderr" else stdout
            with path.open("a") as output:
                output.write(text)
                output.flush()

        result = None
        try:
            with environment.scoped_output_callback(capture):
                result = await environment.exec(
                    command=" ".join(shlex.quote(part) for part in command),
                    env={"HOME": "/tmp/summoner-home"},
                )
        finally:
            if result is not None:
                if stdout.stat().st_size == 0:
                    stdout.write_text(result.stdout or "")
                if stderr.stat().st_size == 0:
                    stderr.write_text(result.stderr or "")
                (self.logs_dir / "summoner.exit-code.txt").write_text(
                    f"{result.return_code}\n"
                )
            self._jsonl(stdout, self.logs_dir / "summoner.stdout.jsonl", context)
            await self._sessions(environment)

    def _jsonl(self, source: Path, target: Path, context: AgentContext) -> None:
        usage = {"prompt_tokens": 0, "cached_tokens": 0, "completion_tokens": 0}
        with source.open() as raw, target.open("w") as clean:
            for line in raw:
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                clean.write(line)
                if event.get("type") != "usage":
                    continue
                for field in usage:
                    usage[field] += int(event.get(field, 0))
        context.n_input_tokens = usage["prompt_tokens"]
        context.n_cache_tokens = usage["cached_tokens"]
        context.n_output_tokens = usage["completion_tokens"]

    async def _sessions(self, environment: BaseEnvironment) -> None:
        remote = "/tmp/summoner-home/.mage/sessions"
        try:
            if await environment.is_dir(remote):
                await environment.download_dir(remote, self.logs_dir / "sessions")
        except Exception as error:
            (self.logs_dir / "summoner-session-capture-error.txt").write_text(
                f"{type(error).__name__}: {error}\n"
            )
