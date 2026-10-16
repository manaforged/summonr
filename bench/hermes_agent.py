import json
from typing import override

import level
import yaml
from pydantic import Field
from harbor.agents.installed.hermes import Hermes, HermesOptions
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

INSTALLER = "https://raw.githubusercontent.com/NousResearch/hermes-agent/{}/scripts/install.sh"
PATH = 'export PATH="$HOME/.local/bin:$PATH"'


class HermesCliOptions(HermesOptions):
    reasoning_effort: str | None = Field(default=None, description="Hermes agent.reasoning_effort.")


class HermesCli(Hermes):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC
    options_model = HermesCliOptions
    options: HermesCliOptions

    def _build_config_yaml(self, model: str, max_turns: int | None = None) -> str:
        config = yaml.safe_load(Hermes._build_config_yaml(model, max_turns))
        config.pop("delegation", None)
        if self.options.reasoning_effort:
            config.setdefault("agent", {})["reasoning_effort"] = self.options.reasoning_effort
        return yaml.dump(config, default_flow_style=False)

    def trial_env(self) -> dict[str, str]:
        base = self._get_env("BENCH_BASE")
        return {"GLM_BASE_URL": level.per_trial(base, self.logs_dir.parent.name)} if base else {}

    @override
    def get_version_command(self) -> str | None:
        return f"{PATH}; hermes --version"

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        branch = f" --branch {self._version}" if self._version else ""
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"curl -fsSL {INSTALLER.format(self._version or 'main')} | bash -s -- --skip-setup{branch} && "
                f"{PATH} && "
                'export HERMES_HOME="${HERMES_HOME:-/tmp/hermes}" && '
                'mkdir -p "$HERMES_HOME" "$HERMES_HOME/sessions" "$HERMES_HOME/skills" "$HERMES_HOME/memories" && '
                f"hermes pm repair && hermes --version && {level.wrap('hermes', self.trial_env())}"
            ),
        )
        await self.exec_as_agent(environment, command=level.snapshot())

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        path = self.logs_dir / "hermes-session.jsonl"
        if context.n_input_tokens or not path.exists():
            return
        sessions = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
        total = lambda key: sum(session.get(key) or 0 for session in sessions)
        context.n_cache_tokens = total("cache_read_tokens")
        context.n_input_tokens = total("input_tokens") + context.n_cache_tokens + total("cache_write_tokens")
        context.n_output_tokens = total("output_tokens")
