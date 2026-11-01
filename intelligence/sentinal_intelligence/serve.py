"""`serve`: read allowed screens, translate them locally, propose Events (6.2).

Each pass lists the Sessions the owner lets this agent read, fetches each
screen (already stripped and redacted by the worker), and lets the
translator decide whether to call the model: never for an unchanged or
blank screen, at most once per Session per interval. Calls that pass the
local guards are proposed; the control checks them again.
"""

from __future__ import annotations

import sys
import time
from typing import Any

from .control import AgentApi, ControlError
from .translator import Translator


def run_once(api: AgentApi, translator: Translator) -> list[dict[str, Any]]:
    report: list[dict[str, Any]] = []
    for grant in api.output_sessions():
        machine_id, session_id = grant["machine_id"], grant["session_id"]
        key = f"{machine_id}/{session_id}"
        try:
            screen = api.screen(machine_id, session_id)
        except ControlError as error:
            report.append({"session": key, "error": str(error)})
            continue
        proposal = translator.propose(key, screen.get("command") or "", "\n".join(screen.get("lines", [])))
        if proposal is None or not proposal.events:
            continue
        events = [{"function": event["function"], "args": event["args"]} for event in proposal.events]
        try:
            reply = api.propose(machine_id, session_id, events)
        except ControlError as error:
            report.append({"session": key, "error": str(error)})
            continue
        report.append({"session": key, "proposed": len(events), "accepted": len(reply.get("accepted", [])),
                       "rejected_locally": len(proposal.rejected)})
    return report


def serve_forever(api: AgentApi, translator: Translator, interval: float, log: Any = sys.stderr) -> None:
    while True:
        try:
            for line in run_once(api, translator):
                print(line, file=log, flush=True)
        except ControlError as error:
            print({"error": str(error)}, file=log, flush=True)
        time.sleep(interval)
