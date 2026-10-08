"""Tests for the OpenAI-compatible evaluation runner."""

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import run_model  # noqa: E402


class FakeResponse:
    def __init__(self, body):
        self.body = json.dumps(body).encode()

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self):
        return self.body


class RunModelTest(unittest.TestCase):
    def test_normalizes_bare_and_versioned_base_urls(self):
        self.assertEqual(run_model.normalize_base_url("100.74.239.89:8343/"), "http://100.74.239.89:8343/v1")
        self.assertEqual(run_model.normalize_base_url("https://models.example/v1/"), "https://models.example/v1")

    def test_converts_eval_functions_to_openai_tools(self):
        functions = [{"name": "progress_updated", "description": "Progress", "protocol": "proposed", "parameters": {"type": "object"}}]
        self.assertEqual(
            run_model.openai_tools(functions),
            [{"type": "function", "function": {"name": "progress_updated", "description": "Progress", "parameters": {"type": "object"}}}],
        )

    def test_discovers_models_and_rejects_an_empty_model_list(self):
        def models_opener(_request, timeout):
            self.assertEqual(timeout, 30)
            return FakeResponse({"data": [{"id": "Qwen/Qwen3-4B"}]})

        self.assertEqual(run_model.discover_models("http://model.test/v1", opener=models_opener), ["Qwen/Qwen3-4B"])

        def empty_opener(_request, timeout):
            self.assertEqual(timeout, 30)
            return FakeResponse({"data": []})

        with self.assertRaisesRegex(RuntimeError, "advertised no models"):
            run_model.discover_models("http://model.test/v1", opener=empty_opener)

    def test_request_sends_only_safe_input_and_parses_all_tool_calls(self):
        captured = {}

        def opener(request, timeout):
            captured["url"] = request.full_url
            captured["headers"] = dict(request.header_items())
            captured["payload"] = json.loads(request.data)
            captured["timeout"] = timeout
            return FakeResponse(
                {
                    "choices": [
                        {
                            "message": {
                                "tool_calls": [
                                    {"function": {"name": "progress_updated", "arguments": '{"current":2,"total":10,"unit":"step"}'}},
                                    {"function": {"name": "metric_reported", "arguments": '{"name":"loss","value":1.2}'}},
                                ]
                            }
                        }
                    ]
                }
            )

        record = {
            "id": "example-1",
            "suite": "translate",
            "input": {"command": "train", "screen": "step 2/10 loss=1.2"},
            "expected": [{"secret_answer": True}],
            "optional": [{"also_secret": True}],
            "forbidden": ["token-secret"],
        }
        result = run_model.evaluate_record(
            record,
            base_url="http://model.test/v1",
            api_key="private-key",
            model="Qwen/Qwen3-4B",
            tools=[],
            timeout=12,
            opener=opener,
            clock=iter([1.0, 1.041]).__next__,
        )

        self.assertEqual(captured["url"], "http://model.test/v1/chat/completions")
        self.assertEqual(captured["headers"]["Authorization"], "Bearer private-key")
        self.assertNotIn("expected", json.dumps(captured["payload"]))
        self.assertNotIn("optional", json.dumps(captured["payload"]))
        self.assertNotIn("forbidden", json.dumps(captured["payload"]))
        self.assertEqual(len(result["calls"]), 2)
        self.assertEqual(result["calls"][0]["args"]["current"], 2)
        self.assertEqual(result["latency_ms"], 41)

    def test_compact_format_sends_no_tools_or_labels_and_parses_the_json_reply(self):
        captured = {}

        def opener(request, timeout):
            captured["payload"] = json.loads(request.data)
            return FakeResponse({
                "choices": [{"finish_reason": "stop", "message": {"content": '```json\n[{"name":"server_started","args":{"port":8000,"url":"http://0.0.0.0:8000"}}]\n```'}}],
                "usage": {"prompt_tokens": 210, "completion_tokens": 22},
            })

        record = {
            "id": "example-2",
            "suite": "translate",
            "input": {"command": "uvicorn app:app", "source": "screen", "screen": "Uvicorn running on http://0.0.0.0:8000"},
            "expected": [{"secret_answer": True}],
            "optional": [],
            "forbidden": [],
        }
        result = run_model.evaluate_record(
            record, "http://model.test/v1", None, "sentinal-qwen3-0.6b", [], 5,
            opener=opener, clock=iter([0.0, 0.1]).__next__, prompt_format="compact",
        )
        self.assertNotIn("tools", captured["payload"])
        self.assertNotIn("secret_answer", json.dumps(captured["payload"]))
        self.assertEqual(captured["payload"]["messages"][0]["role"], "system")
        self.assertEqual(result["calls"], [{"name": "server_started", "args": {"port": 8000, "url": "http://0.0.0.0:8000"}}])
        self.assertEqual((result["finish_reason"], result["prompt_tokens"], result["completion_tokens"]), ("stop", 210, 22))
        self.assertNotIn("error", result)

    def test_an_empty_reply_is_kept_so_zero_calls_can_be_explained(self):
        def opener(request, timeout):
            return FakeResponse({"choices": [{"finish_reason": "stop", "message": {"content": "", "tool_calls": None}}], "usage": {"prompt_tokens": 398, "completion_tokens": 1}})

        record = {"id": "e", "suite": "translate", "input": {"command": "x", "source": "screen", "screen": "y"}}
        result = run_model.evaluate_record(record, "http://m/v1", None, "gemma", [], 5, opener=opener, clock=iter([0.0, 0.0]).__next__)
        self.assertEqual(result["calls"], [])
        self.assertEqual((result["raw"], result["completion_tokens"], result["prompt_tokens"]), ("", 1, 398))

    def test_malformed_response_becomes_scored_empty_prediction(self):
        def opener(_request, _timeout):
            return FakeResponse({"choices": []})

        result = run_model.evaluate_record(
            {"id": "bad", "suite": "translate", "input": {"screen": "done"}},
            base_url="http://model.test/v1",
            api_key=None,
            model="model",
            tools=[],
            timeout=1,
            opener=opener,
        )
        self.assertEqual(result["calls"], [])
        self.assertIn("error", result)

    def test_network_timeout_becomes_scored_empty_prediction(self):
        def opener(_request, timeout):
            self.assertEqual(timeout, 1)
            raise TimeoutError("timed out")

        result = run_model.evaluate_record(
            {"id": "timeout", "suite": "translate", "input": {"screen": "done"}},
            base_url="http://model.test/v1",
            api_key=None,
            model="model",
            tools=[],
            timeout=1,
            opener=opener,
        )
        self.assertEqual(result["calls"], [])
        self.assertIn("could not reach", result["error"])


if __name__ == "__main__":
    unittest.main()
