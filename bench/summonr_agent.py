import hashlib
import json
import shlex
from pathlib import Path, PurePosixPath
from typing import Any, override

import image_user
import level
from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.base import BaseInstalledAgent, CliFlag, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trajectories import (
    Agent,
    FinalMetrics,
    Metrics,
    Observation,
    ObservationResult,
    Step,
    ToolCall,
    Trajectory,
)
from harbor.models.trial.paths import EnvironmentPaths
from harbor.utils.trajectory_utils import format_trajectory_json

ENDPOINTS = {"openai": "openai", "xai": "grok", "zai": "zai"}
LOGINS = PurePosixPath("/opt/auth/summonr")
LOGIN_FILES = ("credentials.toml", "credentials.wrap", "credentials.lock", "locks")
BINARY = "/usr/local/bin/summonr"


class Summonr(BaseInstalledAgent):
    DEFAULT_SETUP_TIMEOUT_SEC = level.SETUP_TIMEOUT_SEC
    capabilities = AgentCapabilities(atif=True, skills=True, mcp_servers=True)

    _OUTPUT_FILENAME = "summonr.txt"
    _instruction = ""

    CLI_FLAGS = [
        CliFlag(
            "reasoning_effort",
            cli="--effort",
            type="enum",
            choices=["minimal", "low", "medium", "high", "xhigh", "max"],
        ),
    ]

    def __init__(self, *args, binary: str | None = None, binary_sha256: str | None = None, **kwargs):
        super().__init__(*args, **kwargs)
        if not binary or not Path(binary).is_file():
            raise ValueError(f"binary must name a summonr build on the host, got {binary!r}")
        if not binary_sha256:
            raise ValueError("binary_sha256 must pin the summonr build for the whole job")
        self._binary = Path(binary)
        self._binary_sha256 = binary_sha256

    @staticmethod
    @override
    def name() -> str:
        return "summonr"

    @override
    def get_version_command(self) -> str | None:
        return f"{BINARY} --version"

    @override
    def parse_version(self, stdout: str) -> str:
        return stdout.split()[1]

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        found = hashlib.sha256(self._binary.read_bytes()).hexdigest()
        if found != self._binary_sha256:
            raise ValueError(f"{self._binary} has sha256 {found}, but the job pins {self._binary_sha256}")
        await self.exec_as_root(environment, command=level.record())
        await level.install(self, environment)
        await environment.upload_file(self._binary, BINARY)
        await self.exec_as_root(environment, command=f"chmod 755 {BINARY} && {BINARY} --version")
        await self.exec_as_agent(environment, command=level.snapshot())

    def _endpoint(self) -> tuple[str, str]:
        provider, _, model = (self.model_name or "").partition("/")
        if provider not in ENDPOINTS or not model:
            raise ValueError(f"model must be one of {sorted(ENDPOINTS)}/MODEL, got {self.model_name!r}")
        return ENDPOINTS[provider], model

    @override
    @with_prompt_template
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        self._instruction = instruction
        endpoint, model = self._endpoint()
        home = (EnvironmentPaths.agent_dir / "summonr").as_posix()
        marker = f"{home}/.started"
        logins = await image_user.lend(self, environment, LOGINS)
        links = " ".join(f"ln -sfn {logins / name} {home}/{name};" for name in LOGIN_FILES)
        base, key = self._get_env("BENCH_BASE"), self._get_env("BENCH_KEY")
        config = f'[endpoint.{endpoint}]\nmodel = "{model}"\n' + (f'base_url = "{level.per_trial(base, self.logs_dir.parent.name)}"\n' if base else "") + (f'api_key = "{key}"\n' if base and key else "")
        for server in self.mcp_servers:
            launch = f"command = {json.dumps(server.command)}\nargs = {json.dumps(server.args)}\n" if server.transport == "stdio" else f"url = {json.dumps(server.url)}\n"
            config += f"[mcp.{server.name}]\n{launch}"
        skills = f"mkdir -p {home}/skills && cp -r {shlex.quote(self.skills_dir)}/* {home}/skills/ 2>/dev/null; " if self.skills_dir else ""
        await self.exec_as_agent(
            environment,
            command=(
                f"mkdir -p {home} {logins}/locks && touch {marker} && "
                f"printf %s {shlex.quote(config)} > {home}/config.toml && {links} {skills}true"
            ),
        )
        flags = self.build_cli_flags()
        env = {**self.extra_env, "SUMMONR_HOME": home, "SUMMONR_SECRETS": "file"}
        try:
            await self.exec_as_agent(
                environment,
                command=(
                    f"{BINARY} -e {endpoint} --json {flags} -p -- {shlex.quote(instruction)} "
                    f"2>{EnvironmentPaths.agent_dir}/summonr.err </dev/null "
                    f"| tee {EnvironmentPaths.agent_dir / self._OUTPUT_FILENAME}"
                ),
                env=env,
            )
        finally:
            await image_user.give_back(self, environment, LOGINS, logins)
            await self.exec_as_root(
                environment,
                command=(
                    f"find {home} -maxdepth 1 -type l -delete; "
                    f"find {LOGINS} -maxdepth 1 -newer {marker} ! -user \"$(stat -c %u {LOGINS})\" "
                    f"-exec chown --reference={LOGINS} {{}} + ; true"
                ),
            )

    def _events(self) -> list[dict[str, Any]]:
        path = self.logs_dir / self._OUTPUT_FILENAME
        if not path.is_file():
            return []
        events = []
        for line in path.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict):
                events.append(event)
        return events

    def _trajectory(self, events: list[dict[str, Any]]) -> Trajectory:
        session = next((e.get("id") for e in events if e.get("type") == "session"), None)
        steps: list[Step] = []
        calls: dict[str, Step] = {}
        text: list[str] = []
        reasoning: list[str] = []
        pending: list[ToolCall] = []
        totals = {"prompt": 0, "completion": 0, "cached": 0, "cost": 0.0}
        for event in events:
            kind = event.get("type")
            if kind == "assistant":
                text.append(event.get("text") or "")
            elif kind == "reasoning":
                reasoning.append(event.get("text") or "")
            elif kind == "tool_call":
                try:
                    arguments = json.loads(event.get("arguments") or "{}")
                except json.JSONDecodeError:
                    arguments = {"raw_arguments": event.get("arguments")}
                if not isinstance(arguments, dict):
                    arguments = {"raw_arguments": arguments}
                pending.append(ToolCall(tool_call_id=event["id"], function_name=event["name"], arguments=arguments))
            elif kind == "tool_result" and event.get("id") in calls:
                step = calls[event["id"]]
                result = ObservationResult(source_call_id=event["id"], content=event.get("output"))
                step.observation = step.observation or Observation(results=[])
                step.observation.results.append(result)
            elif kind == "usage":
                totals["prompt"] += event.get("prompt_tokens", 0)
                totals["completion"] += event.get("completion_tokens", 0)
                totals["cached"] += event.get("cached_tokens", 0)
                totals["cost"] += event.get("cost") or 0.0
                step = Step(
                    step_id=len(steps) + 2,
                    source="agent",
                    message="\n".join(part for part in text if part),
                    reasoning_content="\n".join(part for part in reasoning if part) or None,
                    tool_calls=pending or None,
                    metrics=Metrics(
                        prompt_tokens=event.get("prompt_tokens"),
                        completion_tokens=event.get("completion_tokens"),
                        cached_tokens=event.get("cached_tokens"),
                        cost_usd=event.get("cost"),
                    ),
                )
                calls.update({call.tool_call_id: step for call in pending})
                steps.append(step)
                text, reasoning, pending = [], [], []
        return Trajectory(
            session_id=session,
            agent=Agent(name=self.name(), version=self._version or "unknown", model_name=self.model_name),
            steps=[Step(step_id=1, source="user", message=self._instruction or ""), *steps],
            final_metrics=FinalMetrics(
                total_prompt_tokens=totals["prompt"],
                total_completion_tokens=totals["completion"],
                total_cached_tokens=totals["cached"],
                total_cost_usd=totals["cost"] or None,
                total_steps=len(steps) + 1,
            ),
        )

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        events = self._events()
        if not events:
            return
        trajectory = self._trajectory(events)
        (self.logs_dir / "trajectory.json").write_text(format_trajectory_json(trajectory.to_json_dict()))
        metrics = trajectory.final_metrics
        context.n_input_tokens = metrics.total_prompt_tokens
        context.n_cache_tokens = metrics.total_cached_tokens
        context.n_output_tokens = metrics.total_completion_tokens
        context.cost_usd = metrics.total_cost_usd


class SummonrControl(Summonr):
    @staticmethod
    @override
    def name() -> str:
        return "summonr-control"
