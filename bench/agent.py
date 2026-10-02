import hashlib
import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

PROXY_KEY = "bench-proxy"
VERSIONS = Path(os.environ.get("BENCH_VERSIONS", Path(__file__).resolve().parent / "tools" / "versions.txt"))


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
        task = self.logs_dir.parent.name.split("__", 1)[0]
        group = hashlib.sha256(f"{env['BENCH_RUN']}.{task}".encode()).hexdigest()[:12]
        prompt = "/logs/agent/instruction.txt"
        await environment.exec(f"cat > {prompt} <<'BENCH_EOF'\n{instruction}\nBENCH_EOF")
        variables = {
            "BENCH_BASE": f"{env['BENCH_PROXY']}/{group}~{label}/v1",
            "BENCH_MODEL": env.get("BENCH_MODEL", "deepseek-flash"),
            "BENCH_PROVIDER": env.get("BENCH_PROVIDER", "deepseek"),
            "BENCH_API_KEY": PROXY_KEY,
            "DEEPSEEK_API_KEY": PROXY_KEY,
            "ZAI_API_KEY": PROXY_KEY,
            "BENCH_LOGS": "/logs/agent",
        }
        for name in ("BENCH_EFFORT", "BENCH_MAX_TOKENS", "BENCH_KEEP_REASONING", "SUMMONR_BODY"):
            if env.get(name):
                variables[name] = env[name]
        command = (
            f"bash /opt/bench/harness.sh {shlex.quote(harness)} {prompt}"
            f" > /logs/agent/{harness}.out 2> /logs/agent/{harness}.err;"
            f" echo $? > /logs/agent/exit"
        )
        await environment.exec(command, env=variables)
