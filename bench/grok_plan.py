import shlex
from pathlib import PurePosixPath
from typing import override

from harbor.agents.installed.base import with_prompt_template
from harbor.agents.installed.grok_build import GrokBuild
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trial.paths import EnvironmentPaths

LOGINS = PurePosixPath("/opt/auth/grok")


class GrokPlan(GrokBuild):
    @override
    @with_prompt_template
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        marker = f"/tmp/.grok-plan-{self._session_id}"
        env = {
            "GROK_AUTH_PATH": (LOGINS / "auth.json").as_posix(),
            "GROK_DISABLE_AUTOUPDATER": "1",
            "GROK_LOG_FILE": (EnvironmentPaths.agent_dir / self._CLI_LOG_FILENAME).as_posix(),
            "RUST_LOG": self._get_env("RUST_LOG") or "warn",
        }
        await self.exec_as_agent(environment, command=f"touch {marker} && {self._build_write_config_command()}")
        try:
            await self.exec_as_agent(
                environment,
                command=self._build_run_script(shlex.quote(instruction)).replace("--single ", "--single=", 1),
                env=env,
            )
        finally:
            await self.exec_as_root(
                environment,
                command=(
                    f"find {LOGINS} -maxdepth 1 -newer {marker} ! -user \"$(stat -c %u {LOGINS})\" "
                    f"-exec chown --reference={LOGINS} {{}} + ; true"
                ),
            )
            sessions = EnvironmentPaths.agent_dir / "sessions"
            await self.exec_as_agent(
                environment,
                command=(
                    f"mkdir -p {EnvironmentPaths.agent_dir}\n"
                    'if [ -d "$HOME/.grok/sessions" ]; then\n'
                    f"  rm -rf {sessions}\n"
                    f'  cp -R "$HOME/.grok/sessions" {sessions}\n'
                    "fi"
                ),
            )
