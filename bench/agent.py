import hashlib
import json
import os
import shlex
import tomllib
from pathlib import Path
from typing import override

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

VERSIONS = Path(os.environ.get("BENCH_VERSIONS", Path(__file__).resolve().parent / "tools" / "versions.txt"))


def budget(trial: Path) -> int | None:
    config = trial / "config.json"
    if not config.is_file():
        return None
    data = json.loads(config.read_text())
    task = Path(data["task"]["path"]) / "task.toml"
    if not task.is_file():
        return None
    timeout = tomllib.loads(task.read_text()).get("agent", {}).get("timeout_sec")
    if not timeout:
        return None
    return max(60, int(timeout * data.get("agent_timeout_multiplier", 1.0)) - 30)


class Bench(BaseAgent):
    @staticmethod
    @override
    def name() -> str:
        return "bench"

    @override
    def version(self) -> str:
        harness = self._extra_env.get("BENCH_HARNESS", "")
        if harness == "summonr":
            binary = VERSIONS.parent / "summonr" / "bin" / "summonr"
            if binary.is_file():
                return hashlib.sha256(binary.read_bytes()).hexdigest()[:16]
        for line in VERSIONS.read_text().splitlines():
            key, _, value = line.partition(" ")
            if key == harness:
                return value
        return "unknown"

    @override
    async def setup(self, environment: BaseEnvironment) -> None:
        await environment.exec("mkdir -p /logs/agent")

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        env = self._extra_env
        harness = env["BENCH_HARNESS"]
        label = hashlib.sha256(f"{harness}.{env['BENCH_RUN']}.{self.logs_dir.parent.name}".encode()).hexdigest()[:16]
        prompt = "/logs/agent/instruction.txt"
        await environment.exec(f"cat > {prompt} <<'BENCH_EOF'\n{instruction}\nBENCH_EOF")
        key = os.environ.get("BENCH_API_KEY") or os.environ["DEEPSEEK_API_KEY"]
        variables = {
            "BENCH_BASE": f"{env['BENCH_PROXY']}/{label}/v1",
            "BENCH_MODEL": env.get("BENCH_MODEL", "deepseek-flash"),
            "BENCH_PROVIDER": env.get("BENCH_PROVIDER", "deepseek"),
            "BENCH_API_KEY": key,
            "DEEPSEEK_API_KEY": key,
            "BENCH_LOGS": "/logs/agent",
        }
        if os.environ.get("ZAI_API_KEY"):
            variables["ZAI_API_KEY"] = os.environ["ZAI_API_KEY"]
        for name in ("BENCH_EFFORT", "BENCH_MAX_TOKENS", "BENCH_KEEP_REASONING", "SUMMONR_BODY"):
            if env.get(name):
                variables[name] = env[name]
        seconds = budget(self.logs_dir.parent)
        if seconds:
            variables["BENCH_MAX_SECS"] = str(seconds)
        command = (
            f"bash /opt/bench/harness.sh {shlex.quote(harness)} {prompt}"
            f" > /logs/agent/{harness}.out 2> /logs/agent/{harness}.err;"
            f" echo $? > /logs/agent/exit"
        )
        await environment.exec(command, env=variables)
