"""Tests for the compact prompt format shared by fine-tuning and evaluation."""

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import compact  # noqa: E402
import generate  # noqa: E402
from score import score  # noqa: E402


class CompactTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.records = generate.build("test", "compact-test", 0.1, include_heldout=True)

    def test_parsing_the_training_target_reproduces_a_perfect_prediction(self):
        predictions = {}
        for record in self.records:
            calls, error = compact.parse_reply(compact.render_target(record))
            self.assertIsNone(error)
            predictions[record["id"]] = {"calls": calls}
        report, failures = score(self.records, predictions)
        self.assertEqual(report["overall"]["f1"], 1.0, failures[:2])

    def test_prompt_never_contains_the_answer_or_labels(self):
        for record in self.records:
            turns = compact.messages(record)
            self.assertEqual([turn["role"] for turn in turns], ["system", "user"])
            if record["expected"]:
                self.assertNotIn(compact.render_target(record), json.dumps(turns))

    def test_recipe_history_shows_exit_codes(self):
        record = next(r for r in self.records if r["suite"] == "recipes")
        self.assertIn("[exit ", compact.render_user(record))

    def test_reply_parsing_tolerates_fences_thinking_and_prose_but_reports_garbage(self):
        fenced = '<think>\n\n</think>\n```json\n[{"name":"warning_detected","args":{}}]\n```'
        self.assertEqual(compact.parse_reply(fenced), ([{"name": "warning_detected", "args": {}}], None))
        self.assertEqual(compact.parse_reply("Here you go: []"), ([], None))
        self.assertEqual(compact.parse_reply("no idea")[1], "reply has no JSON array")
        self.assertEqual(compact.parse_reply("[{broken")[1], "reply has no JSON array")
        self.assertEqual(compact.parse_reply("[1, 2]")[1], "reply has non-object items")


if __name__ == "__main__":
    unittest.main()
