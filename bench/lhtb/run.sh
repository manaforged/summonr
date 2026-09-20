#!/usr/bin/env bash
set -euo pipefail

lhtb=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$lhtb")
run=${1:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
tasks=${2:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
shift 2
harnesses=("${@:-summonr pi omp opencode2 hermes goose}")
read -ra harnesses <<< "${harnesses[*]}"
seed=${BENCH_SEED:-20260916}
effort=${BENCH_EFFORT:-}
concurrent=${BENCH_CONCURRENT:-2}
repeats=${BENCH_REPEATS:-1}
timeout=${BENCH_TIMEOUT:-5400}
dataset=${BENCH_DATASET:-$root/datasets/lhtb}
relay=gogost/gost@sha256:afc0137758ab4ce399d47a299f9abbacbf522b52a17e59cbb4b4e7a1a66e9196
harbor=$lhtb/venv/bin/harbor
python=$lhtb/venv/bin/python
jobs=$root/jobs/$run
export PYTHONPATH=$lhtb
set -a; . "$root/deepseek.env"; set +a

mkdir -p "$jobs/configs" "$jobs/relay"
cp "$root/tools/versions.txt" "$root/tools/manifest.sha256" "$lhtb/harness.sh" "$lhtb/agent.py" "$tasks" "$jobs/"
package=$("$python" -c 'import harbor, os; print(os.path.dirname(harbor.__file__))')
{
  echo "seed $seed"
  echo "effort ${effort:-default}"
  echo "repeats $repeats"
  echo "concurrent $concurrent"
  echo "override_timeout_sec $timeout"
  echo "harbor $("$harbor" --version)"
  echo "single_step.py $(sha256sum "$package/trial/single_step.py" | cut -d' ' -f1)"
  echo "lhtb_dropin $(sha256sum "$dataset/harbor/patches/single_step.py.harbor-0.20.0" | cut -d' ' -f1)"
  echo "freeze.patch $(sha256sum "$lhtb/freeze.patch" | cut -d' ' -f1)"
  echo "feedback $("$python" -c 'import harbor.trial.single_step as m; print(hasattr(m, "_AGENT_TREE_SIGNAL_CMD"), m._resolve_verifier_feedback_mode())')"
  echo "dataset $(git -C "$dataset" rev-parse HEAD) dirty=$(git -C "$dataset" status --porcelain | wc -l)"
  echo "harness.sh $(sha256sum "$lhtb/harness.sh" | cut -d' ' -f1)"
  echo "agent.py $(sha256sum "$lhtb/agent.py" | cut -d' ' -f1)"
  echo "tools $(sha256sum "$root/tools/manifest.sha256" | cut -d' ' -f1)"
  echo "relay $relay"
} > "$jobs/run.txt"

"$python" - "$tasks" "$seed" "$repeats" "$run" "$dataset/tasks" "$jobs" "$root" "$lhtb" "$timeout" "$effort" "$relay" "${harnesses[@]}" > "$jobs/order.txt" <<'EOF'
import json, random, sys
from pathlib import Path
import yaml
tasks_file, seed, repeats, run, tasks_dir, jobs, root, lhtb, timeout, effort, relay, *harnesses = sys.argv[1:]
tasks = [line.strip() for line in open(tasks_file) if line.strip()]
jobs = Path(jobs)
for task in tasks:
    networks = ["default"]
    compose = Path(tasks_dir, task, "environment", "docker-compose.yaml")
    if compose.exists():
        main = (yaml.safe_load(compose.read_text()).get("services") or {}).get("main") or {}
        declared = main.get("networks") or []
        networks += [name for name in declared if name != "default"]
    service = {"image": relay, "command": ["-L", "tcp://:8094/172.17.0.1:8094"], "restart": "no", "networks": networks}
    (jobs / "relay" / f"{task}.yaml").write_text(yaml.safe_dump({"services": {"bench-relay": service}}))
triples = [(task, harness, rep) for rep in range(1, int(repeats) + 1) for task in tasks for harness in harnesses]
random.Random(int(seed)).shuffle(triples)
for task, harness, rep in triples:
    name = f"{harness}.{task}.r{rep}"
    env = {"BENCH_HARNESS": harness, "BENCH_RUN": run, "BENCH_PROXY": "http://bench-relay:8094"}
    if effort:
        env["BENCH_EFFORT"] = effort
    config = {
        "job_name": name,
        "jobs_dir": str(jobs),
        "n_concurrent_trials": 1,
        "environment": {
            "type": "docker",
            "mounts": [
                {"type": "bind", "source": f"{root}/tools", "target": "/opt/bench", "read_only": True},
                {"type": "bind", "source": f"{lhtb}/harness.sh", "target": "/opt/lhtb/harness.sh", "read_only": True},
            ],
            "extra_docker_compose": [str(jobs / "relay" / f"{task}.yaml")],
        },
        "agents": [{
            "import_path": "agent:Bench",
            "override_timeout_sec": float(timeout),
            "extra_allowed_hosts": ["bench-relay"],
            "env": env,
        }],
        "datasets": [{"path": tasks_dir, "task_names": [task]}],
    }
    (jobs / "configs" / f"{name}.json").write_text(json.dumps(config, indent=2))
    print(name)
EOF

trial() {
  local name=$1
  [ -f "$jobs/$name/result.json" ] && return 0
  "$harbor" run -c "$jobs/configs/$name.json" > "$jobs/$name.log" 2>&1 || echo "harbor failed: $name" >> "$jobs/failures.txt"
  echo "$(date -u +%FT%TZ) done $name"
}
export -f trial
export harbor jobs
xargs -P "$concurrent" -L 1 bash -c 'trial "$0"' < "$jobs/order.txt"
{
  docker image inspect "$relay" --format '{{.Id}} {{index .RepoDigests 0}}'
  while read -r task; do
    image=$(sed -n 's/^docker_image *= *"\(.*\)"/\1/p' "$dataset/tasks/$task/task.toml")
    [ -n "$image" ] && docker image inspect "$image" --format "$task {{.Id}} {{json .RepoDigests}}" 2>/dev/null || echo "$task ${image:-built} unknown"
  done < "$tasks"
} > "$jobs/images.txt"
