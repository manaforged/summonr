import asyncio
import re
from pathlib import Path

from harbor.models.job.plugin import BaseJobPlugin
from harbor.trial.hooks import TrialHookEvent

PROCESSES = {
    "summonr": ("summonr",),
    "codex": ("codex",),
    "grok-build": ("grok",),
    "claude-code": ("claude",),
}
SHELL = re.compile(r"^(ba|da|z|k|a)?sh$")
INTERVAL = 0.5


def project(trial_name):
    name = f"{trial_name}__env".lower()
    if not re.match(r"^[a-z0-9]", name):
        name = "0" + name
    return re.sub(r"[^a-z0-9_-]", "-", name)


def cgroup(pid):
    return Path(f"/proc/{pid}/cgroup").read_text()


def table(group):
    procs = {}
    for status in Path("/proc").glob("[0-9]*/status"):
        try:
            if cgroup(status.parent.name) != group:
                continue
            fields = dict(line.split(":", 1) for line in status.read_text().splitlines() if ":" in line)
            argv = (status.parent / "cmdline").read_bytes().split(b"\0")[:2]
        except OSError:
            continue
        procs[int(fields["Pid"])] = {
            "parent": int(fields["PPid"]),
            "name": fields["Name"].strip(),
            "argv": [Path(arg.decode(errors="replace")).name for arg in argv if arg],
            "kb": int(fields.get("VmRSS", "0 kB").split()[0]),
        }
    return procs


def sample(group, names):
    procs = table(group)
    children = {}
    for pid, proc in procs.items():
        children.setdefault(proc["parent"], []).append(pid)

    def mine(proc):
        return any(word.startswith(names) for word in [proc["name"], *proc["argv"]])

    own = tree = helpers = 0
    stack = [(pid, False, False) for pid, proc in procs.items() if proc["parent"] not in procs]
    while stack:
        pid, inside, shelled = stack.pop()
        proc = procs.get(pid)
        if proc is None:
            continue
        inside = inside or mine(proc)
        if inside:
            tree += proc["kb"]
            own += proc["kb"] if mine(proc) else 0
            shelled = shelled or bool(SHELL.match(proc["name"]))
            helpers += 0 if shelled else proc["kb"]
        stack.extend((child, inside, shelled) for child in children.get(pid, []))
    return own, tree, helpers


async def container_pid(name):
    ps = await asyncio.create_subprocess_exec(
        "docker", "ps", "-q", "--filter", f"label=com.docker.compose.project={project(name)}",
        "--filter", "label=com.docker.compose.service=main", stdout=asyncio.subprocess.PIPE,
    )
    ids = (await ps.communicate())[0].decode().split()
    if not ids:
        return None
    inspect = await asyncio.create_subprocess_exec(
        "docker", "inspect", "-f", "{{.State.Pid}}", ids[0], stdout=asyncio.subprocess.PIPE,
    )
    return int((await inspect.communicate())[0].decode().strip() or 0) or None


class Rss(BaseJobPlugin):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._tasks = {}

    async def on_job_start(self, job):
        job.on_agent_started(self._start)
        job.on_agent_ended(self._stop)

    async def on_job_end(self, job_result):
        for task in self._tasks.values():
            task.cancel()

    def _names(self, event):
        agent = event.config.agent
        key = agent.name or (agent.import_path or "").split(":")[0].removesuffix("_agent")
        key = {"grok_plan": "grok-build"}.get(key, key)
        return PROCESSES.get(key)

    async def _start(self, event: TrialHookEvent):
        names = self._names(event)
        init = await container_pid(event.trial_name)
        if names and init:
            out = event.config.trials_dir / event.trial_name / "memory.txt"
            self._tasks[event.trial_name] = asyncio.create_task(self._watch(cgroup(init), names, out))

    async def _stop(self, event: TrialHookEvent):
        task = self._tasks.pop(event.trial_name, None)
        if task:
            task.cancel()

    async def _watch(self, group, names, out):
        peak = [0, 0, 0]
        total = [0, 0, 0]
        samples = 0
        while True:
            values = await asyncio.to_thread(sample, group, names)
            if values[0]:
                samples += 1
                peak = [max(a, b) for a, b in zip(peak, values)]
                total = [a + b for a, b in zip(total, values)]
                out.write_text(
                    "".join(
                        f"{key}_kb {value}\n"
                        for key, value in zip(
                            ("agent", "tree", "harness", "agent_avg", "tree_avg", "harness_avg"),
                            [*peak, *(value // samples for value in total)],
                        )
                    )
                )
            await asyncio.sleep(INTERVAL)
