from typing import override

import level
from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment

PATH = 'export PATH="$HOME/.local/bin:$PATH"; '
PASSED = ("CLAUDE_CODE_MAX_OUTPUT_TOKENS",)


class Claude(ClaudeCode):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        await super().install(environment)
        await self.exec_as_agent(environment, command=f"{PATH}{level.wrap('claude')}")
        await self.exec_as_agent(environment, command=level.snapshot())

    @override
    def _resolve_auth_env(self) -> dict[str, str]:
        env = super()._resolve_auth_env()
        if base := self._get_env("BENCH_BASE"):
            env["ANTHROPIC_BASE_URL"] = level.per_trial(base, self.logs_dir.parent.name)
        env.update({name: value for name in PASSED if (value := self._get_env(name))})
        return env
