"""Tests for the FunctionGemma prompt and reply format."""

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import functiongemma as fg  # noqa: E402
import generate  # noqa: E402
from score import score  # noqa: E402


class FakeTemplateTokenizer:
    """Stands in for the real tokenizer: renders turns with this module's own call syntax."""

    def apply_chat_template(self, messages, tools=None, tokenize=False, add_generation_prompt=False):
        text = "<bos>" + "".join(self._turn(m) for m in messages)
        return text + ("<start_of_turn>model\n" if add_generation_prompt else "")

    @staticmethod
    def _turn(message):
        if message["role"] == "assistant":
            calls = [{"name": c["function"]["name"], "args": c["function"]["arguments"]} for c in message.get("tool_calls", [])]
            return "<start_of_turn>model\n" + fg.render_calls(calls) + message.get("content", "") + "<end_of_turn>\n"
        return f"<start_of_turn>{message['role']}\n{message['content']}<end_of_turn>\n"


class FunctionGemmaFormatTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.records = generate.build("test", "functiongemma-test", 0.1, include_heldout=True)
        cls.dev = generate.build("dev", "functiongemma-test", 0.1, include_heldout=False)

    def test_parsing_the_rendered_target_reproduces_a_perfect_prediction(self):
        predictions = {}
        for record in self.records:
            calls, error = fg.parse_reply(fg.render_target(record))
            self.assertIsNone(error, fg.render_target(record))
            predictions[record["id"]] = {"calls": calls}
        report, failures = score(self.records, predictions)
        self.assertEqual(report["overall"]["f1"], 1.0, failures[:2])

    def test_values_render_in_function_gemma_syntax(self):
        text = fg.render_calls([{"name": "progress_updated", "args": {"current": 12, "total": None, "unit": "epoch"}}])
        self.assertEqual(
            text, "<start_function_call>call:progress_updated{current:12,total:null,unit:<escape>epoch<escape>}<end_function_call>"
        )

    def test_nested_objects_and_arrays_round_trip(self):
        args = {"name": "train", "template": "python train.py --lr {lr}", "params": [{"name": "lr", "default": "3e-4"}], "ok": True}
        calls, error = fg.parse_reply(fg.render_calls([{"name": "propose_recipe", "args": args}]))
        self.assertIsNone(error)
        self.assertEqual(calls, [{"name": "propose_recipe", "args": args}])

    def test_escaped_strings_keep_braces_commas_and_colons(self):
        args = {"summary": "KeyError: 'x', in {train}", "kind": "exception"}
        calls, _ = fg.parse_reply(fg.render_calls([{"name": "error_detected", "args": args}]))
        self.assertEqual(calls[0]["args"], args)

    def test_several_calls_in_one_reply(self):
        reply = fg.render_calls([{"name": "warning_detected", "args": {"summary": "a"}}, {"name": "metric_reported", "args": {"name": "loss", "value": 0.5}}])
        calls, error = fg.parse_reply(reply)
        self.assertIsNone(error)
        self.assertEqual([call["name"] for call in calls], ["warning_detected", "metric_reported"])
        self.assertEqual(calls[1]["args"]["value"], 0.5)

    def test_empty_reply_means_no_calls(self):
        self.assertEqual(fg.parse_reply(""), ([], None))
        self.assertEqual(fg.parse_reply("<end_of_turn>"), ([], None))

    def test_prose_without_a_call_is_reported(self):
        calls, error = fg.parse_reply("I think the training is at epoch 3.")
        self.assertEqual(calls, [])
        self.assertEqual(error, "reply has no function call")

    def test_broken_call_is_dropped_and_reported(self):
        good = fg.render_calls([{"name": "warning_detected", "args": {"summary": "a"}}])
        calls, error = fg.parse_reply(good + "<start_function_call>call:metric_reported{name:<escape>loss")
        self.assertEqual([call["name"] for call in calls], ["warning_detected"])
        self.assertEqual(error, "reply has a malformed function call")

    def test_reply_is_never_executed_or_evaluated(self):
        calls, _ = fg.parse_reply("<start_function_call>call:warning_detected{summary:__import__('os').system('id')}<end_function_call>")
        self.assertEqual(calls[0]["args"]["summary"], "__import__('os').system('id')")

    def test_prompt_declares_tools_and_never_contains_the_answer(self):
        tools = fg.tools()
        self.assertIn("progress_updated", [tool["function"]["name"] for tool in tools])
        for record in self.records:
            turns = fg.messages(record)
            self.assertEqual([turn["role"] for turn in turns], ["developer", "user"])
            self.assertTrue(turns[0]["content"].startswith(fg.DEVELOPER_PREFIX))
            if record["expected"]:
                self.assertNotIn(fg.render_target(record), json.dumps(turns))

    def test_training_rows_come_from_the_tokenizer_template_and_check_out(self):
        tokenizer = FakeTemplateTokenizer()
        record = next(r for r in self.dev if len(r["expected"]) >= 2)
        row = fg.training_row(record, tokenizer)
        self.assertTrue(row["prompt"].endswith("<start_of_turn>model\n"))
        self.assertEqual(row["completion"], fg.render_target(record) + "<end_of_turn>\n")

    def test_training_row_refuses_a_template_this_parser_cannot_read(self):
        class OtherSyntax(FakeTemplateTokenizer):
            @staticmethod
            def _turn(message):
                if message["role"] == "assistant":
                    return "<start_of_turn>model\n<tool_call>{}</tool_call><end_of_turn>\n"
                return FakeTemplateTokenizer._turn(message)

        record = next(r for r in self.dev if r["expected"])
        with self.assertRaises(fg.TemplateMismatch):
            fg.training_row(record, OtherSyntax())

    def test_every_dev_record_survives_the_template_check(self):
        tokenizer = FakeTemplateTokenizer()
        for record in self.dev:
            fg.training_row(record, tokenizer)

    def test_training_script_drops_the_template_bos_the_trainer_adds_again(self):
        sys.path.insert(0, str(ROOT / "finetune"))
        import train_sft

        tokenizer = FakeTemplateTokenizer()
        tokenizer.bos_token = "<bos>"
        rows = train_sft.to_prompt_completion(self.dev[:5], tokenizer, "functiongemma")
        self.assertTrue(all(not row["prompt"].startswith("<bos>") for row in rows))
        self.assertTrue(all(row["prompt"].startswith("<start_of_turn>developer") for row in rows))

    def test_training_row_refuses_test_records(self):
        record = dict(self.records[0], split="test")
        with self.assertRaises(SystemExit):
            fg.training_row(record, FakeTemplateTokenizer(), refuse_test=True)


if __name__ == "__main__":
    unittest.main()
