import base64

TASK_PATH = "/etc/bench-task-path"
SETUP_TIMEOUT_SEC = 1200.0
PACKAGES = ("ca_certificates", "curl", "git", "ripgrep", "xz", "unzip", "bash", "coreutils", "python3", "nodejs", "npm")
RC_DIR = "/etc/bench-rc"
DEBIAN_NAMES = {"ca_certificates": "ca-certificates", "xz": "xz-utils"}
SNAPSHOT_LIST = "/etc/apt/bench-snapshot.list"
HOMES = "/root /home/*"
RC_FILES = (".bashrc", ".profile", ".bash_profile", ".bash_login", ".zshrc", ".zprofile")
APT_DIR = "/etc/apt"
APT_CACHE_HOST = "host.docker.internal"
APT_CACHE_PORT = 3142


HOSTS = (
    'h=$(awk \'$2=="00000000"{print $3; exit}\' /proc/net/route) && '
    'gw=$(printf "%d.%d.%d.%d" 0x$(echo $h | cut -c7-8) 0x$(echo $h | cut -c5-6) 0x$(echo $h | cut -c3-4) 0x$(echo $h | cut -c1-2)) && '
    '{ grep -q " host.docker.internal$" /etc/hosts || echo "$gw host.docker.internal" >> /etc/hosts; }'
)


def keep() -> str:
    save = f'for h in {HOMES}; do for f in {" ".join(RC_FILES)}; do [ ! -e "$h/$f" ] || cp -p "$h/$f" "{RC_DIR}/$(echo "$h/$f" | tr / _)"; done; done'
    return f'[ -s {TASK_PATH} ] || {{ printf %s "$PATH" > {TASK_PATH}; mkdir -p {RC_DIR}; {save}; }}'


def apt_cache() -> str:
    probe = f"#!/bin/bash\n(exec 3<>/dev/tcp/{APT_CACHE_HOST}/{APT_CACHE_PORT}) 2>/dev/null && echo http://{APT_CACHE_HOST}:{APT_CACHE_PORT} || echo DIRECT\n"
    conf = f'Acquire::http::Proxy-Auto-Detect "{APT_DIR}/bench-proxy";\nDPkg::Options {{"--force-unsafe-io";}};\n'
    encode = lambda text: base64.b64encode(text.encode()).decode()
    return (
        f"[ ! -d {APT_DIR}/apt.conf.d ] || {{ echo {encode(probe)} | base64 -d > {APT_DIR}/bench-proxy && chmod 755 {APT_DIR}/bench-proxy && "
        f"echo {encode(conf)} | base64 -d > {APT_DIR}/apt.conf.d/00bench; }}"
    )


def record() -> str:
    return f"{keep()}; {HOSTS}; {apt_cache()}"


def recipe() -> str:
    return "\n".join((record(), " ".join(PACKAGES), snapshot_install()))


def restore() -> str:
    return f'for f in {" ".join(RC_FILES)}; do b="{RC_DIR}/$(echo "$HOME/$f" | tr / _)"; if [ -e "$b" ]; then cp "$b" "$HOME/$f"; else rm -f "$HOME/$f"; fi; done'


def per_trial(base: str, trial: str) -> str:
    head, arm, tail = base.rsplit("/", 2)
    return f"{head}/{arm}~{trial}/{tail}"


def wrap(name: str, env: dict[str, str] | None = None) -> str:
    extra = "".join(f" {key}={value}" for key, value in (env or {}).items())
    return (
        f'bin=$(command -v {name}) && real="$bin.real" && mv "$bin" "$real" && '
        'line=$(head -n 1 "$(readlink -f "$real")" | tr -d "\\000\\r") && '
        'case "$line" in '
        "'#!/usr/bin/env '*) rt=$(command -v ${line#\\#!/usr/bin/env }) ;; "
        "'#!'*) rt=${line#\\#!} ;; "
        "*) rt= ;; esac && "
        f"printf '#!/bin/sh\\nexec env PATH=\"$(cat {TASK_PATH})\"{extra} %s\"%s\" \"$@\"\\n' \"${{rt:+$rt }}\" \"$real\" > \"$bin\" && "
        'chmod 755 "$bin"'
    )



PROBE = """echo "PATH=$PATH"
for r in python3 pip3 node npm npx bun uv perl ruby go java gcc; do echo "$r=$(command -v $r)"; done
echo "dpkg=$(dpkg-query -W -f '${Package}\\n' 2>/dev/null | sort | sha256sum | cut -c1-16)"
"""


def snapshot(path: str = "/logs/agent/parity.txt") -> str:
    script = base64.b64encode(PROBE.encode()).decode()
    return f'{restore()}; echo {script} | base64 -d | env PATH="$(cat {TASK_PATH})" sh > {path}'


def snapshot_install() -> str:
    names = " ".join(DEBIAN_NAMES.get(name, name) for name in PACKAGES)
    flags = f"-o Dir::Etc::SourceList={SNAPSHOT_LIST} -o Dir::Etc::SourceParts=/dev/null -o Acquire::Check-Valid-Until=false"
    return (
        f"sed -n 's/^# *\\(deb http:\\/\\/snapshot.debian.org.*\\)/\\1/p' /etc/apt/sources.list > {SNAPSHOT_LIST} && [ -s {SNAPSHOT_LIST} ] && "
        f"apt-get {flags} update -qq && DEBIAN_FRONTEND=noninteractive apt-get {flags} install -y -qq {names}"
    )


async def install(agent, environment) -> None:
    try:
        await agent.ensure_system_dependencies(environment, PACKAGES)
    except Exception:
        await agent.exec_as_root(environment, command=snapshot_install())
    if keep := getattr(environment, "keep_level", None):
        await keep()
