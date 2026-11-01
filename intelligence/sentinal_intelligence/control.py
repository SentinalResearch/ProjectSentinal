"""The control's agent API, as an agent sees it (6.0, 6.2). Standard library only."""

from __future__ import annotations

import json
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen

TOKEN_PREFIX = "sat_"


class ControlError(RuntimeError):
    """The control refused a request or could not be reached."""


class AgentApi:
    def __init__(self, origin: str, token: str, opener: Any = urlopen, timeout: float = 15.0):
        if not origin.startswith(("https://", "http://")):
            raise ValueError("the control origin must start with https:// (or http:// in development)")
        if not token.startswith(TOKEN_PREFIX):
            raise ValueError("this is not an agent token; create one in Settings → Agents")
        self.origin, self.token, self.opener, self.timeout = origin.rstrip("/"), token, opener, timeout

    def _call(self, method: str, path: str, body: Any = None) -> Any:
        data = None if body is None else json.dumps(body).encode()
        headers = {"Authorization": f"Bearer {self.token}", "Accept": "application/json"}
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = Request(self.origin + path, data=data, headers=headers, method=method)
        try:
            with self.opener(request, timeout=self.timeout) as response:
                return json.loads(response.read().decode("utf-8"))
        except HTTPError as error:
            try:
                message = json.loads(error.read().decode("utf-8")).get("message", error.reason)
            except (ValueError, AttributeError):
                message = error.reason
            raise ControlError(f"{error.code} {message}") from error
        except (URLError, OSError, ValueError) as error:
            raise ControlError(f"could not reach the control: {error}") from error

    def output_sessions(self) -> list[dict[str, Any]]:
        return list(self._call("GET", "/api/v1/agent/output-sessions")["grants"])

    def _session(self, machine_id: str, session_id: int) -> str:
        return f"/api/v1/agent/machines/{quote(machine_id, safe='')}/sessions/{int(session_id)}"

    def screen(self, machine_id: str, session_id: int) -> dict[str, Any]:
        return dict(self._call("GET", self._session(machine_id, session_id) + "/screen"))

    def propose(self, machine_id: str, session_id: int, events: list[dict[str, Any]]) -> dict[str, Any]:
        return dict(self._call("POST", self._session(machine_id, session_id) + "/events", {"events": events}))
