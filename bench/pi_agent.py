import json
import shlex
from typing import override

import level
from harbor.agents.installed.pi import Pi as HarborPi
from harbor.environments.base import BaseEnvironment


class Pi(HarborPi):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC
    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        await super().install(environment)
        await self.exec_as_agent(environment, command=f". ~/.nvm/nvm.sh; {level.wrap('pi')}")
        if base := self._get_env("BENCH_BASE"):
            models = json.dumps({"providers": {"zai": {"baseUrl": level.per_trial(base, self.logs_dir.parent.name)}}})
            await self.exec_as_agent(environment, command=f"mkdir -p ~/.pi/agent && printf %s {shlex.quote(models)} > ~/.pi/agent/models.json")
        await self.exec_as_agent(environment, command=level.snapshot())

    @override
    def build_cli_flags(self) -> str:
        return " ".join(filter(None, [super().build_cli_flags(), "--"]))
