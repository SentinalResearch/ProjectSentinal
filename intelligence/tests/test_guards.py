"""Guards are code, not prompt: tests for sanitising screens and checking model calls."""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "intelligence"))
sys.path.insert(0, str(ROOT / "eval"))

import generate

from sentinal_intelligence import guards, sanitize


class SanitizeTest(unittest.TestCase):
    def test_strips_ansi_and_keeps_only_the_screen_tail(self):
        raw = "\x1b[1;32mEpoch 1/3\x1b[0m\r\n" + "\n".join(f"line {i}" for i in range(200))
        screen = sanitize.clean(raw, max_lines=60).text
        self.assertNotIn("\x1b", screen)
        self.assertEqual(len(screen.splitlines()), 60)
        self.assertTrue(screen.endswith("line 199"))

    def test_caps_characters_from_the_end(self):
        self.assertEqual(len(sanitize.clean("x" * 50_000, max_chars=6000).text), 6000)

    def test_redacts_secret_shapes_before_any_model_sees_them(self):
        secrets = [
            "hf_" + "a" * 34,
            "sk-" + "b" * 48,
            "AKIA" + "C" * 16,
            "ghp_" + "d" * 36,
            "xoxb-123456789012-" + "e" * 24,
        ]
        lines = [
            f"export HF_TOKEN={secrets[0]}",
            f"OPENAI_API_KEY={secrets[1]} python app.py",
            f"aws key {secrets[2]}",
            f"Authorization: Bearer {secrets[3]}",
            f"slack {secrets[4]}",
            "git remote add origin https://oauth2:hunter2pass@gitlab.com/team/repo.git",
            "DATABASE_URL=postgres://admin:Pa55-word@db.internal:5432/app",
            "Open http://localhost:8888/lab?token=abc123def456",
            "wandb: Appending key for api.wandb.ai to your netrc file: Pa55-netrcvalue1",
        ]
        cleaned = sanitize.clean("\n".join(lines))
        for value in secrets + ["hunter2pass", "Pa55-word", "abc123def456", "Pa55-netrcvalue1"]:
            self.assertNotIn(value, cleaned.text)
            self.assertIn(value, cleaned.removed)
        self.assertIn("http://localhost:8888/lab", cleaned.text)
        self.assertIn("db.internal:5432", cleaned.text)

    def test_ordinary_training_output_is_left_alone(self):
        screen = "step 120/500 loss=0.81 lr=3e-4 grad_norm=1.2\nSaved checkpoint to runs/ckpt-120.pt"
        self.assertEqual(sanitize.clean(screen).text, screen)


def _translate(record):
    return record["suite"] == "translate"


class GuardsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.records = generate.build("dev", "guards-test", 1.0, include_heldout=False)

    def check(self, record, calls):
        context = guards.Context.from_record(record)
        return guards.check_calls(calls, context)

    def test_every_correct_answer_in_the_dev_set_passes(self):
        """A guard that rejects right answers costs recall; this pins it at zero false rejections."""
        for record in self.records:
            context = guards.Context.from_record(record, clean=True)
            calls = [{"name": c["name"], "args": c["args"]} for c in record["expected"]]
            accepted, rejected = guards.check_calls(calls, context)
            self.assertEqual(rejected, [], record["id"])
            self.assertEqual(len(accepted), len(calls))

    def test_unknown_function_and_bad_types_are_rejected(self):
        record = next(r for r in self.records if _translate(r))
        _, rejected = self.check(record, [
            {"name": "run_command", "args": {"command": "rm -rf /"}},
            {"name": "tests_summary", "args": {"passed": "many", "failed": 0, "skipped": 0}},
            {"name": "error_detected", "args": {"kind": "meltdown", "summary": "x"}},
            {"name": "progress_updated", "args": {"total": 10, "unit": "epoch"}},
        ])
        reasons = [reason for _, reason in rejected]
        self.assertEqual(reasons, ["unknown function", "passed has the wrong type", "kind is not allowed", "current is missing"])

    def test_extra_arguments_are_dropped_not_passed_through(self):
        record = next(r for r in self.records if _translate(r))
        accepted, _ = self.check(record, [{"name": "warning_detected", "args": {"summary": "slow disk", "shell": "id"}}])
        self.assertEqual(accepted, [{"name": "warning_detected", "args": {"summary": "slow disk"}}])

    def test_numbers_must_appear_on_screen(self):
        record = {"suite": "translate", "input": {"command": "python train.py", "source": "screen", "screen": "Epoch 3/10 loss 0.52"}}
        accepted, rejected = self.check(record, [
            {"name": "progress_updated", "args": {"current": 3, "total": 10, "unit": "epoch"}},
            {"name": "progress_updated", "args": {"current": 7, "total": 10, "unit": "epoch"}},
            {"name": "metric_reported", "args": {"name": "loss", "value": 0.52}},
            {"name": "metric_reported", "args": {"name": "loss", "value": 0.1}},
        ])
        self.assertEqual(len(accepted), 2)
        self.assertEqual([reason for _, reason in rejected], ["current is not on screen", "value is not on screen"])

    def test_checkpoints_and_urls_must_appear_on_screen_and_lose_their_query(self):
        record = {"suite": "translate", "input": {"command": "jupyter lab", "source": "screen",
                                                   "screen": "Saved runs/a.pt\nOpen http://localhost:8888/lab?token=abc123def456"}}
        accepted, rejected = self.check(record, [
            {"name": "checkpoint_created", "args": {"path": "runs/a.pt"}},
            {"name": "checkpoint_created", "args": {"path": "/etc/passwd"}},
            {"name": "server_started", "args": {"port": 8888, "url": "http://localhost:8888/lab?token=abc123def456"}},
            {"name": "server_started", "args": {"port": 8888, "url": "http://evil.example/"}},
        ])
        self.assertEqual(accepted[1]["args"]["url"], "http://localhost:8888/lab")
        self.assertEqual([reason for _, reason in rejected], ["path is not on screen", "url is not on screen"])

    def test_a_prompt_must_be_on_the_last_lines(self):
        record = {"suite": "translate", "input": {"command": "apt upgrade", "source": "screen",
                                                   "screen": "Do you want to continue? [Y/n]\nDone.\n$ "}}
        _, rejected = self.check(record, [{"name": "waiting_for_input", "args": {"kind": "confirm", "prompt": "Do you want to continue? [Y/n]"}}])
        self.assertEqual([reason for _, reason in rejected], ["prompt is not on the last lines"])

    def test_secret_shapes_and_removed_values_never_leave(self):
        record = {"suite": "translate", "input": {"command": "python app.py", "source": "screen",
                                                   "screen": "export HF_TOKEN=hf_" + "a" * 34 + "\nWARNING: slow"}}
        context = guards.Context.from_record(record, clean=True)
        _, rejected = guards.check_calls([
            {"name": "warning_detected", "args": {"summary": "token hf_" + "a" * 34}},
            {"name": "warning_detected", "args": {"summary": "key sk-" + "z" * 40}},
        ], context)
        self.assertEqual([reason for _, reason in rejected], ["contains a secret", "contains a secret"])

    def test_history_is_cleaned_too(self):
        record = {"suite": "recipes", "input": {"history": [{"command": "export HF_TOKEN=Pa55-abcdefghijklmn", "cwd": "~", "exit_code": 0}]}}
        context = guards.Context.from_record(record)
        self.assertNotIn("Pa55-abcdefghijklmn", context.text)
        self.assertIn("Pa55-abcdefghijklmn", context.removed)

    def test_no_known_secret_reaches_the_model_on_the_dev_set(self):
        for record in self.records:
            context = guards.Context.from_record(record)
            for secret in record["forbidden"]:
                self.assertNotIn(secret, context.text, record["id"])

    def test_summaries_are_capped(self):
        record = next(r for r in self.records if _translate(r))
        _, rejected = self.check(record, [{"name": "warning_detected", "args": {"summary": "w" * 121}}])
        self.assertEqual([reason for _, reason in rejected], ["summary is too long"])

    def test_recipes_only_come_from_history(self):
        record = next(r for r in self.records if _translate(r))
        _, rejected = self.check(record, [{"name": "propose_recipe", "args": {"name": "x", "template": "make", "params": []}}])
        self.assertEqual([reason for _, reason in rejected], ["not allowed for this input"])


if __name__ == "__main__":
    unittest.main()
