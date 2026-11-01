"""The service loop against the control's agent API (6.2), with a fake control."""

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "intelligence"))

from sentinal_intelligence import formats
from sentinal_intelligence.control import AgentApi, ControlError
from sentinal_intelligence.serve import run_once
from sentinal_intelligence.translator import Translator

FG = formats.functiongemma
MACHINE = "01J9Z3K8V6Q2T7M4X5C0RNB8DA"
TOKEN = "sat_agt_00aa11bb22cc33dd_" + "f" * 64


class FakeResponse:
    def __init__(self, status, body):
        self.status, self.body = status, json.dumps(body).encode()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False

    def read(self):
        return self.body


class FakeControl:
    """Serves one allowed Session and records proposals."""

    def __init__(self, lines):
        self.lines, self.proposed, self.requests = lines, [], []

    def __call__(self, request, timeout):
        self.requests.append((request.get_method(), request.full_url, dict(request.header_items())))
        path = request.full_url.split("://", 1)[1].split("/", 1)[1]
        if path == "api/v1/agent/output-sessions":
            return FakeResponse(200, {"grants": [{"machine_id": MACHINE, "session_id": 7}]})
        if path.endswith("/screen"):
            return FakeResponse(200, {"machine_id": MACHINE, "session_id": 7, "command": "python train.py",
                                      "lines": self.lines, "redacted": 0})
        if path.endswith("/events"):
            body = json.loads(request.data)
            self.proposed.append(body)
            return FakeResponse(200, {"accepted": [{"function": e["function"]} for e in body["events"]], "rejected": []})
        raise AssertionError(path)


def model_replying(calls):
    def model(record):
        return {"calls": FG.parse_reply(FG.render_calls(calls))[0], "latency_ms": 5}
    return model


class Clock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now


class ServeTest(unittest.TestCase):
    def test_a_screen_becomes_proposals_sent_with_the_agent_token(self):
        control = FakeControl(["Epoch 12/40 loss=0.81"])
        api = AgentApi("http://127.0.0.1:7421", TOKEN, opener=control)
        translator = Translator(model_replying([
            {"name": "progress_updated", "args": {"current": 12, "total": 40, "unit": "epoch"}},
            {"name": "metric_reported", "args": {"name": "loss", "value": 9.9}},
        ]), clock=Clock())
        report = run_once(api, translator)
        self.assertEqual(control.proposed, [{"events": [
            {"function": "progress_updated", "args": {"current": 12, "total": 40, "unit": "epoch"}}]}])
        self.assertEqual(report, [{"session": f"{MACHINE}/7", "proposed": 1, "accepted": 1, "rejected_locally": 1}])
        self.assertTrue(all(headers.get("Authorization") == f"Bearer {TOKEN}" for _, _, headers in control.requests))

    def test_nothing_is_sent_when_the_model_finds_nothing_or_the_screen_is_unchanged(self):
        control = FakeControl(["$ ls", "README.md"])
        api = AgentApi("http://127.0.0.1:7421", TOKEN, opener=control)
        clock = Clock()
        translator = Translator(model_replying([]), clock=clock)
        run_once(api, translator)
        clock.now += 60
        run_once(api, translator)
        self.assertEqual(control.proposed, [])

    def test_a_refusal_from_the_control_is_reported_not_raised(self):
        def refusing(request, timeout):
            import io
            from urllib.error import HTTPError
            raise HTTPError(request.full_url, 403, "Forbidden", {}, io.BytesIO(b'{"code":"UNAUTHORIZED","message":"this agent may not read terminal output"}'))

        api = AgentApi("http://127.0.0.1:7421", TOKEN, opener=refusing)
        with self.assertRaises(ControlError) as caught:
            api.output_sessions()
        self.assertIn("may not read terminal output", str(caught.exception))

    def test_only_agent_tokens_and_safe_origins_are_accepted(self):
        with self.assertRaises(ValueError):
            AgentApi("http://127.0.0.1:7421", "a" * 64)
        with self.assertRaises(ValueError):
            AgentApi("ftp://control", TOKEN)


if __name__ == "__main__":
    unittest.main()
