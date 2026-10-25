import shutil
import subprocess
import sys
from pathlib import Path

TASKS = Path(__file__).resolve().parent.parent / "tasks-hard"
SCORE = Path(__file__).resolve().parent / "score.py"


def main():
    work = Path(sys.argv[1]).resolve()
    work.mkdir(parents=True)
    lines = ["# Tasks", "", "Each section names one directory. Finish every one.", ""]
    for task in sorted(path for path in TASKS.iterdir() if (path / "verify.sh").is_file()):
        shutil.copytree(task / "fixture", work / task.name)
        lines += [f"## {task.name}", "", (task / "task.txt").read_text().strip(), ""]
    (work / "TASKS.md").write_text("\n".join(lines))
    (work / ".summonr").mkdir()
    (work / ".summonr" / "check.toml").write_text(f'check = "python3 -I {SCORE} {TASKS} ."\n')
    for command in (["git", "init", "-q"], ["git", "add", "-A"]):
        subprocess.run(command, cwd=work, check=True)
    print(work)


if __name__ == "__main__":
    main()
