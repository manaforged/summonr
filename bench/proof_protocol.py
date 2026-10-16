import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import paired
import table


def row(harness, task, prompt, reward, rss=100.0):
    return {
        "harness": harness, "task": task, "reward": reward, "void": [],
        "prompt": prompt, "miss": prompt, "output": 10, "cost": None, "agent_seconds": 60.0,
        "first_prompt": 1000, "harness_avg_mb": rss, "harness_mb": rss * 2,
    }


class Protocol(unittest.TestCase):
    def test_tokens_per_solved_try_leave_out_failed_tries_and_memory_is_paired(self):
        rows = [
            row("summonr", "fix-git", 100, 1.0, rss=50.0),
            row("summonr", "fix-git", 10000, 0.0, rss=50.0),
            row("pi", "fix-git", 200, 1.0),
            row("pi", "fix-git", 200, 1.0),
        ]
        line = paired.paired(rows, "summonr", samples=200).splitlines()[-1]
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        self.assertEqual(cells[:4], ["pi", "1", "-0.50 (-0.50–-0.50)", "0.50 (0.50–0.50)"])
        self.assertEqual(cells[-2:], ["0.50 (0.50–0.50)", "0.50 (0.50–0.50)"])

    def test_solve_rate_intervals_resample_tasks_not_tries(self):
        rows = [row("pi", "fix-git", 100, 1.0) for _ in range(5)] + [row("pi", "chess-best-move", 100, 0.0)]
        self.assertEqual(table.rate(rows), "5/6, 0.50 (0.00–1.00)")
        self.assertEqual(table.rate(rows, any), "5/6, 0.50 (0.00–1.00)")


class Control(unittest.TestCase):
    def test_the_control_holds_when_twin_arms_match_and_fails_when_one_drifts(self):
        tasks = [f"task-{index}" for index in range(30)]
        same = [row(arm, task, 1000 + index, 1.0 if index % 3 else 0.0) for arm in ("summonr", "summonr-control") for index, task in enumerate(tasks)]
        self.assertTrue(paired.control(same, "summonr", "summonr-control", samples=500).endswith("A/A control holds."))
        drift = [{**r, "prompt": r["prompt"] * 1.3} if r["harness"] == "summonr-control" else r for r in same]
        report = paired.control(drift, "summonr", "summonr-control", samples=500)
        self.assertIn("- input tokens per solved try: 0.769 (0.769–0.769), width 0.000", report)
        self.assertTrue(report.endswith("A/A control fails."))



class Window(unittest.TestCase):
    def test_each_arm_prints_the_hours_it_ran(self):
        rows = [
            {"harness": "pi", "window": ("2026-10-16T01:00:00Z", "2026-10-16T01:20:00Z")},
            {"harness": "pi", "window": ("2026-10-16T09:00:00Z", "2026-10-16T09:30:00Z")},
            {"harness": "summonr", "window": ("2026-10-16T01:05:00Z", "2026-10-16T07:00:00Z")},
            {"harness": "summonr", "window": (None, None)},
        ]
        self.assertEqual(table.windows(rows), "Active window per arm (UTC): pi 2026-10-16T01:00 to 2026-10-16T09:30; summonr 2026-10-16T01:05 to 2026-10-16T07:00.")


if __name__ == "__main__":
    unittest.main()
