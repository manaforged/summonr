import hashlib
import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

VERSIONS = Path(os.environ.get("BENCH_VERSIONS", "tools/versions.txt"))


class Bench(BaseAgent):
    @staticmethod
    @override
    def name() -> str:
        return "bench"

    @override
    def version(self) -> str:
        harness = self._extra_env.get("BENCH_HARNESS", "")
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
        variables = {
            "BENCH_BASE": f"{env['BENCH_PROXY']}/{label}/v1",
            "BENCH_MODEL": env.get("BENCH_MODEL", "glm-5.3-flash"),
            "ZAI_API_KEY": os.environ["ZAI_API_KEY"],
            "BENCH_LOGS": "/logs/agent",
        }
        if env.get("BENCH_EFFORT"):
            variables["BENCH_EFFORT"] = env["BENCH_EFFORT"]
        command = (
            f"bash /opt/bench/harness.sh {shlex.quote(harness)} {prompt}"
            f" > /logs/agent/{harness}.out 2> /logs/agent/{harness}.err;"
            f" echo $? > /logs/agent/exit"
        )
        await environment.exec(command, env=variables)
