import asyncio
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import level


class Tmp(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())


class Startup(Tmp):
    def test_an_installers_shell_startup_edits_are_gone_before_the_model_runs(self):
        home = self.tmp / "root"
        home.mkdir()
        (home / ".bashrc").write_text("alias ll='ls -l'\n")
        level.TASK_PATH, level.RC_DIR, level.HOMES = str(self.tmp / "task-path"), str(self.tmp / "rc"), str(home)
        env = {"HOME": str(home), "PATH": "/usr/bin:/bin"}
        subprocess.run(["bash", "-c", level.keep()], env=env, check=True)
        (home / ".bashrc").write_text("alias ll='ls -l'\nexport PATH=\"$HOME/.bun/bin:$PATH\"\n")
        (home / ".profile").write_text(". ~/.bashrc\n")
        subprocess.run(["bash", "-c", level.restore()], env=env, check=True)
        self.assertEqual(((home / ".bashrc").read_text(), (home / ".profile").exists()), ("alias ll='ls -l'\n", False))


class Level(Tmp):
    def apt(self, *dirs):
        root = self.tmp / "etc-apt"
        root.mkdir()
        for d in dirs:
            (root / d).mkdir(parents=True)
        level.APT_DIR = str(root)
        record = level.record()
        subprocess.run(["bash", "-c", record[record.index(level.apt_cache()):]], check=True)
        return root

    def test_apt_goes_through_the_cache_while_it_listens_and_direct_when_it_does_not(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        level.APT_CACHE_HOST, level.APT_CACHE_PORT = "127.0.0.1", listener.getsockname()[1]
        root = self.apt("apt.conf.d")
        probe = lambda: subprocess.run([str(root / "bench-proxy")], capture_output=True, text=True, check=True).stdout
        up = probe()
        listener.close()
        conf = (root / "apt.conf.d" / "00bench").read_text()
        self.assertEqual((up, probe()), (f"http://127.0.0.1:{level.APT_CACHE_PORT}\n", "DIRECT\n"))
        self.assertEqual(conf, f'Acquire::http::Proxy-Auto-Detect "{root}/bench-proxy";\nDPkg::Options {{"--force-unsafe-io";}};\n')

    def test_an_image_without_apt_gets_no_apt_files(self):
        root = self.apt()
        self.assertEqual(list(root.iterdir()), [])


class Install(unittest.TestCase):
    class Agent:
        def __init__(self, fails):
            self.fails, self.sent = fails, []

        async def ensure_system_dependencies(self, environment, packages):
            self.sent.append(("packages", packages))
            if self.fails:
                raise RuntimeError("E: Failed to fetch curl_7.74.0-1.3+deb11u16_amd64.deb 404 Not Found")

        async def exec_as_root(self, environment, command):
            self.sent.append(("root", command))

    def test_a_mirror_that_lost_its_packages_falls_back_to_the_images_pinned_snapshot(self):
        agent = self.Agent(fails=True)
        asyncio.run(level.install(agent, None))
        (_, packages), (kind, command) = agent.sent
        self.assertEqual((packages, kind), (level.PACKAGES, "root"))
        self.assertIn("snapshot.debian.org", command)
        self.assertTrue(command.endswith("install -y -qq ca-certificates curl git ripgrep xz-utils unzip bash coreutils python3 nodejs npm"))

    def test_a_working_mirror_installs_once(self):
        agent = self.Agent(fails=False)
        asyncio.run(level.install(agent, None))
        self.assertEqual(agent.sent, [("packages", level.PACKAGES)])


if __name__ == "__main__":
    unittest.main()
