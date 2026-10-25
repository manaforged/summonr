import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SKIP = shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache")


def judged(task, target):
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch) / task.name
        shutil.copytree(task / "fixture", root, ignore=SKIP)
        for command in (["git", "init", "-q"], ["git", "add", "-A"]):
            subprocess.run(command, cwd=root, check=True, capture_output=True)
        for entry in root.iterdir():
            if entry.name != ".git":
                shutil.rmtree(entry) if entry.is_dir() else entry.unlink()
        shutil.copytree(target, root, ignore=SKIP, dirs_exist_ok=True)
        done = subprocess.run(["bash", str(task / "verify.sh")], cwd=root, capture_output=True, timeout=300)
        return int(done.returncode == 0)


def main():
    tasks, work = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    results = {}
    for task in sorted(path for path in tasks.iterdir() if (path / "verify.sh").is_file()):
        target = work / task.name
        results[task.name] = judged(task, target) if target.is_dir() else 0
    print(json.dumps({"passed": sum(results.values()), **results}))
    sys.exit(0 if all(results.values()) else 1)


if __name__ == "__main__":
    main()
