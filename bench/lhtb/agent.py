import asyncio
import hashlib
import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

VERSIONS = Path.home() / "bench" / "tools" / "versions.txt"
ADAPTER = "/opt/lhtb/harness.sh"
SCAN = r"""
base=$(cat /var/lib/bench/baseline 2>/dev/null)
agents=""
if [ -n "$base" ]; then
  for d in /proc/[0-9]*; do
    pid=${d#/proc/}
    p=$pid
    owner=agent
    while [ -n "$p" ] && [ "$p" != 0 ] && [ "$p" != 1 ]; do
      [ "$p" = "$$" ] && { owner=self; break; }
      case " $base " in *" $p "*) owner=task; break;; esac
      p=$(sed -n 's/^PPid:[[:space:]]*//p' "/proc/$p/status" 2>/dev/null)
    done
    [ "$pid" != 1 ] && [ "$owner" = agent ] && agents="$agents $pid"
  done
fi
"""
KILL = SCAN + 'for pid in $agents; do kill -KILL "$pid" 2>/dev/null; done\nexit 0\n'
BASELINE = "mkdir -p /var/lib/bench && for d in /proc/[0-9]*; do printf '%s ' \"${d#/proc/}\"; done > /var/lib/bench/baseline"


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
        self._phase = 0
        await environment.exec("mkdir -p /logs/agent")
        await environment.exec(BASELINE)

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        await self._invoke(instruction, environment, "new")

    async def resume(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        await self._invoke(instruction, environment, "resume")

    async def _invoke(self, instruction: str, environment: BaseEnvironment, mode: str) -> None:
        env = self._extra_env
        harness = env["BENCH_HARNESS"]
        phase = self._phase
        self._phase += 1
        suffix = f".{phase}" if phase else ""
        label = hashlib.sha256(f"{harness}.{env['BENCH_RUN']}.{self.logs_dir.parent.name}".encode()).hexdigest()[:16]
        prompt = f"/logs/agent/instruction{suffix}.txt"
        await environment.exec(f"cat > {prompt} <<'BENCH_EOF'\n{instruction}\nBENCH_EOF")
        variables = {
            "BENCH_BASE": f"{env['BENCH_PROXY']}/{label}/v1",
            "BENCH_MODEL": env.get("BENCH_MODEL", "deepseek-flash"),
            "DEEPSEEK_API_KEY": os.environ["DEEPSEEK_API_KEY"],
            "BENCH_LOGS": "/logs/agent",
        }
        if env.get("BENCH_EFFORT"):
            variables["BENCH_EFFORT"] = env["BENCH_EFFORT"]
        command = (
            f"bash {ADAPTER} {shlex.quote(harness)} {prompt} {mode}"
            f" > /logs/agent/{harness}{suffix}.out 2> /logs/agent/{harness}{suffix}.err;"
            f" echo $? > /logs/agent/exit{suffix}"
        )
        try:
            await environment.exec(command, env=variables)
        except asyncio.CancelledError:
            await environment.exec(KILL)
            raise
