import shlex
from typing import override

from pydantic import Field

import level

from harbor.agents.installed.pi import Pi, PiOptions
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

PACKAGE = "@oh-my-pi/pi-coding-agent"
PATH = 'export PATH="$HOME/.bun/bin:$PATH"; '


class OmpOptions(PiOptions):
    bun: str = Field(description="Bun version that runs OMP.")


class Omp(Pi):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC
    options_model = OmpOptions
    options: OmpOptions
    _OUTPUT_FILENAME = "omp.txt"

    @staticmethod
    @override
    def name() -> str:
        return "omp"

    @override
    def get_version_command(self) -> str | None:
        return f"{PATH}omp --version"

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        version = f"@{self._version}" if self._version else "@latest"
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"curl -fsSL https://bun.sh/install | bash -s bun-v{shlex.quote(self.options.bun)} && "
                f"{PATH}bun install -g {PACKAGE}{version} && omp --version && {level.wrap('omp')}"
            ),
        )
        if base := self._get_env("BENCH_BASE"):
            provider = (self.model_name or "").partition("/")[0]
            models = f"providers:\n  {provider}:\n    baseUrl: {level.per_trial(base, self.logs_dir.parent.name)}\n"
            await self.exec_as_agent(environment, command=f"mkdir -p ~/.omp/agent && printf %s {shlex.quote(models)} > ~/.omp/agent/models.yml")
        await self.exec_as_agent(environment, command=level.snapshot())

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        if not self.model_name or "/" not in self.model_name:
            raise ValueError("model must be PROVIDER/MODEL")
        flags = self.build_cli_flags()
        sessions = shlex.quote((self.environment_logs_dir / self._SESSIONS_DIRECTORY).as_posix())
        output = shlex.quote((self.environment_logs_dir / self._OUTPUT_FILENAME).as_posix())
        self._session_usage_start = {}
        await self.exec_as_agent(
            environment,
            command=(
                f"{PATH}mkdir -p {sessions} && "
                f"omp --print --mode json --yolo --session-dir {sessions} --model {shlex.quote(self.model_name)} "
                f"{flags + ' ' if flags else ''}-- {shlex.quote(instruction)} "
                f"2>&1 </dev/null | grep -v '\"type\":\"message_update\"' | stdbuf -oL tee {output}"
            ),
        )
