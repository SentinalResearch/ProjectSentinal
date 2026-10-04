"""Self-tests for the evaluation dataset and scorer (standard library only).

    python3 -m unittest discover -s eval -p 'test_*.py'
"""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import generate  # noqa: E402
from score import score  # noqa: E402


def dataset():
    return generate.build("test", "self-test", 0.25, include_heldout=True)


def oracle(records):
    return {record["id"]: {"calls": [{**call, "args": {**call["args"], **({"summary": "x"} if call["name"] in ("error_detected", "warning_detected") else {}), **({"name": "Recipe", "params": call["args"]["params"]} if call["name"] == "propose_recipe" else {})}} for call in record["expected"]]} for record in records}


class ScorerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.records = dataset()

    def test_perfect_answers_score_perfectly(self):
        report, failures = score(self.records, oracle(self.records))
        self.assertEqual(report["overall"]["f1"], 1.0, failures[:3])
        self.assertEqual(report["exact_match"], 1.0)
        self.assertEqual(report["invented_value_rate"], 0.0)
        self.assertEqual(report["secret_leak_examples"], 0)
        self.assertEqual(report["recipe_defaults_correct"], 1.0)

    def test_saying_nothing_has_zero_recall_but_clean_negatives(self):
        report, _ = score(self.records, {})
        self.assertEqual(report["overall"]["recall"], 0.0)
        self.assertEqual(report["negative_screens_clean"], 1.0)
        self.assertEqual(report["missing_predictions"], len(self.records))

    def test_secret_leak_invented_value_and_false_alarm_are_caught(self):
        record = next(r for r in self.records if r["forbidden"] and r["suite"] == "translate")
        leak = {"calls": [{"name": "error_detected", "args": {"kind": "other", "summary": record["forbidden"][0]}}]}
        report, _ = score([record], {record["id"]: leak})
        self.assertEqual(report["secret_leak_examples"], 1)
        invented = {"calls": [{"name": "metric_reported", "args": {"name": "loss", "value": 123.456}}]}
        report, _ = score([record], {record["id"]: invented})
        self.assertEqual(report["invented_value_rate"], 1.0)
        self.assertGreaterEqual(report["overall"]["fp"], 1)

    def test_synonyms_and_percent_readings_count_as_correct(self):
        record = {"id": "x", "suite": "translate", "category": "training", "heldout": False, "edges": [], "forbidden": [], "optional": [],
                  "input": {"screen": "Receiving objects:  27% (270/1000)\nlr=2e-4"},
                  "expected": [{"name": "progress_updated", "args": {"current": 270, "total": 1000, "unit": "objects"}},
                               {"name": "metric_reported", "args": {"name": "learning_rate", "value": 0.0002}}]}
        prediction = {"calls": [{"name": "progress_updated", "args": {"current": 27, "total": 100, "unit": "percent"}},
                                {"name": "metric_reported", "args": {"name": "lr", "value": 2e-4}}]}
        report, failures = score([record], {"x": prediction})
        self.assertEqual(report["overall"]["f1"], 1.0, failures)

    def test_optional_calls_are_not_penalised(self):
        record = next(r for r in self.records if r.get("optional"))
        prediction = oracle([record])[record["id"]]
        prediction["calls"].append(record["optional"][0])
        report, _ = score([record], {record["id"]: prediction})
        self.assertEqual(report["overall"]["fp"], 0)


class GeneratorTest(unittest.TestCase):
    def test_same_seed_same_dataset(self):
        first = json.dumps(dataset(), sort_keys=True)
        self.assertEqual(hashlib.sha256(first.encode()).hexdigest(), hashlib.sha256(json.dumps(dataset(), sort_keys=True).encode()).hexdigest())

    def test_no_duplicate_inputs_and_training_split_never_contains_test_inputs(self):
        with tempfile.TemporaryDirectory() as out:
            subprocess.run([sys.executable, str(ROOT / "generate.py"), "--out", out, "--train", "800"], check=True, capture_output=True)
            keys = lambda name: [generate._input_key(json.loads(line)) for line in (Path(out) / name).read_text().splitlines()]  # noqa: E731
            test, train = keys("test.jsonl"), keys("train.jsonl")
            self.assertEqual(len(test), len(set(test)))
            self.assertFalse(set(test) & set(train))

    def test_every_expected_call_is_a_known_valid_function(self):
        from score import valid
        for record in dataset():
            for call in oracle([record])[record["id"]]["calls"]:
                self.assertTrue(valid(call), (record["id"], call))


if __name__ == "__main__":
    unittest.main()
