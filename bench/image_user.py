import contextlib
from pathlib import PurePosixPath

from harbor.environments.docker.docker import DockerEnvironment

LENT = PurePosixPath("/opt/lent")


class Docker(DockerEnvironment):
    image_user = None

    async def start(self, force_build: bool):
        await super().start(force_build)
        found = await self.exec('[ "$(id -u)" = 0 ] || id -un 2>/dev/null || id -u', user=None)
        self.image_user = (found.stdout or "").strip() or None

    @contextlib.contextmanager
    def with_default_user(self, user):
        with super().with_default_user(self.image_user if user is None else user):
            yield


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
