import asyncio
import contextlib
import hashlib
from importlib.metadata import version
from pathlib import PurePosixPath

import level
from harbor.environments.docker.docker import DockerEnvironment, _sanitize_docker_image_name

LENT = PurePosixPath("/opt/lent")
LEVEL_REPO = "bench-level"
COMPOSE_PROJECT_LABEL = "com.docker.compose.project"


class Docker(DockerEnvironment):
    image_user = None
    level_tag = None
    _level_locks: dict[str, asyncio.Lock] = {}

    async def start(self, force_build: bool):
        base = self.task_env_config.docker_image
        if base and (leveled := await level_image(base)) and await engine("image", "inspect", leveled):
            self._env_vars.prebuilt_image_name = leveled
        await super().start(force_build)
        if base and self._env_vars.prebuilt_image_name == base:
            self.level_tag = await level_image(base)
        found = await self.exec('[ "$(id -u)" = 0 ] || id -un 2>/dev/null || id -u', user=None)
        self.image_user = (found.stdout or "").strip() or None

    async def keep_level(self) -> None:
        if not self.level_tag:
            return
        async with self._level_locks.setdefault(self.level_tag, asyncio.Lock()):
            if await engine("image", "inspect", self.level_tag) is None:
                await engine("commit", "--change", f"LABEL {COMPOSE_PROJECT_LABEL}=", await self._platform._resolve_service_container("main"), self.level_tag)

    @contextlib.contextmanager
    def with_default_user(self, user):
        with super().with_default_user(self.image_user if user is None else user):
            yield


async def engine(*args: str) -> str | None:
    process = await asyncio.create_subprocess_exec(*Docker._engine_cmd(*args), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
    out, _ = await process.communicate()
    return out.decode().strip() if process.returncode == 0 else None


async def level_image(base: str) -> str | None:
    found = await engine("image", "inspect", "-f", "{{.Id}}", base)
    if not found:
        return None
    digest = hashlib.sha256(f"{found}\n{level.recipe()}\n{version('harbor')}".encode()).hexdigest()[:16]
    return f"{LEVEL_REPO}/{_sanitize_docker_image_name(base)}:{digest}"


async def lend(agent, environment, logins: PurePosixPath) -> PurePosixPath:
    user = environment.default_user
    if user is None:
        return logins
    copy = LENT / logins.name
    found = await agent.exec_as_root(
        environment,
        command=(
            f"if [ ! -d {logins} ]; then mkdir -p {logins} && chown {user} {logins}; "
            f"else rm -rf {copy} {copy}.orig && mkdir -p {LENT} && cp -a {logins} {copy} && cp -a {logins} {copy}.orig && chown -R {user} {copy} && echo lent; fi"
        ),
    )
    return copy if "lent" in (found.stdout or "") else logins


async def give_back(agent, environment, logins: PurePosixPath, lent: PurePosixPath) -> None:
    if lent == logins:
        return
    await agent.exec_as_root(
        environment,
        command=(
            f'for f in {lent}/*; do n=${{f##*/}}; d={logins}/$n; o={lent}.orig/$n; [ ! -f "$f" ] || cmp -s "$f" "$o" || '
            f'if [ -e "$d" ]; then ! cmp -s "$d" "$o" || cat "$f" > "$d"; elif [ ! -e "$o" ]; then install -m 600 -o "$(stat -c %u {logins})" -g "$(stat -c %g {logins})" "$f" "$d"; fi; done; '
            f"rm -rf {lent} {lent}.orig; true"
        ),
    )
