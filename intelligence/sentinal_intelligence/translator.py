"""Turn a Session's screen into proposed, inferred Events.

Level 1 only: the translator proposes Events and never sends input, runs
anything, or notifies by itself. It calls the model at most once per
Session per `min_interval` seconds, never for an unchanged or blank screen,
and every reply goes through `guards` before it is proposed.
"""

from __future__ import annotations

import hashlib
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any

from . import guards, sanitize

Model = Callable[[dict[str, Any]], dict[str, Any]]


@dataclass
class Proposal:
    session_id: str
    events: list[dict[str, Any]] = field(default_factory=list)
    rejected: list[tuple[Any, str]] = field(default_factory=list)
    error: str | None = None
    latency_ms: int | None = None


class Translator:
    def __init__(self, model: Model, min_interval: float = 10.0, clock: Callable[[], float] = time.monotonic):
        self.model, self.min_interval, self.clock = model, min_interval, clock
        self._last_call: dict[str, float] = {}
        self._last_screen: dict[str, str] = {}

    def propose(self, session_id: str, command: str, screen: str) -> Proposal | None:
        """A proposal, or None when the model was not called (debounced, unchanged, or blank)."""
        cleaned = sanitize.clean(screen)
        if not cleaned.text.strip():
            return None
        digest = hashlib.sha256(f"{command}\0{cleaned.text}".encode()).hexdigest()
        now = self.clock()
        if self._last_screen.get(session_id) == digest:
            return None
        if now - self._last_call.get(session_id, float("-inf")) < self.min_interval:
            return None
        self._last_call[session_id], self._last_screen[session_id] = now, digest

        record = {"id": session_id, "suite": "translate",
                  "input": {"command": sanitize.redact(command).text, "source": "screen", "screen": cleaned.text}}
        result = self.model(record)
        proposal = Proposal(session_id, error=result.get("error"), latency_ms=result.get("latency_ms"))
        if proposal.error and not result.get("calls"):
            return proposal
        context = guards.Context("translate", cleaned.text, cleaned.removed)
        accepted, proposal.rejected = guards.check_calls(result.get("calls", []), context)
        proposal.events = [{"function": call["name"], "args": call["args"], "source": "INFERRED"} for call in accepted]
        return proposal
