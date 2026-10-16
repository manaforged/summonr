import json
import shlex
from typing import override

import level
from harbor.agents.installed.base import NonZeroAgentExitCodeError
from harbor.agents.installed.node_install import nvm_node_install_snippet
from harbor.agents.installed.opencode import OpenCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

PACKAGE = "@opencode/cli"
PROVIDERS = {"zai": "zai-coding-plan"}


class OpenCode2(OpenCode):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC
    @staticmethod
    @override
    def name() -> str:
        return "opencode2"

    @override
    def get_version_command(self) -> str | None:
        return ". ~/.nvm/nvm.sh; opencode2 --version"

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        version = f"@{self._version}" if self._version else "@latest"
        await self.exec_as_agent(
            environment,
            command=f"set -euo pipefail; {nvm_node_install_snippet()} && npm i -g {PACKAGE}{version} && opencode2 --version && {level.wrap('opencode2')}",
        )
        await self.exec_as_agent(environment, command=level.snapshot())

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        self._instruction = instruction
        provider, _, model = (self.model_name or "").partition("/")
        if not model:
            raise ValueError("model must be PROVIDER/MODEL")
        variant = self.options.variant
        target = f"{PROVIDERS.get(provider, provider)}/{model}" + (f"#{variant}" if variant else "")
        env = {
            "XDG_DATA_HOME": (self.environment_logs_dir / "opencode" / "xdg-data").as_posix(),
            "XDG_STATE_HOME": (self.environment_logs_dir / "opencode" / "xdg-state").as_posix(),
        }
        if base := self._get_env("BENCH_BASE"):
            url = level.per_trial(base, self.logs_dir.parent.name)
            env["OPENCODE_CONFIG_CONTENT"] = json.dumps({"provider": {PROVIDERS.get(provider, provider): {"options": {"baseURL": url}}}})
        output = shlex.quote((self.environment_logs_dir / self._OUTPUT_FILENAME).as_posix())
        await self.exec_as_agent(
            environment,
            command=(
                ". ~/.nvm/nvm.sh; "
                f"opencode2 run --standalone --auto --format json --model {shlex.quote(target)} "
                f"-- {shlex.quote(instruction)} 2>&1 </dev/null | stdbuf -oL tee {output}"
            ),
            env=env,
        )
        if messages := self._error_messages():
            raise NonZeroAgentExitCodeError("OpenCode emitted error event(s): " + "; ".join(messages[:3]))
