"""Check every model call before it can become a proposed Event.

A model reply is data. Each call must name a known function, carry the
right argument types and enum values, and be grounded in the screen: every
number, path, URL and prompt it reports must be visible there. Arguments
the schema does not define are dropped. Calls that contain a secret, or a
value removed by `sanitize`, are refused. Nothing here trusts the model.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from . import sanitize
from .formats import canon, functions

SUMMARY_LIMIT = 120
# The training answers leave these out (the scorer never asks for them), so a
# trained model omits them; the daemon fills them in from the screen line.
NOT_REQUIRED = {"error_detected": {"summary"}, "warning_detected": {"summary"}, "propose_recipe": {"name"}}
_FUNCTIONS = functions()
_SUITES = {"propose_recipe": "recipes"}

Call = dict[str, Any]


@dataclass(frozen=True)
class Context:
    """What the model was shown: the suite, the (cleaned) text, and the values removed from it."""

    suite: str
    text: str
    removed: tuple[str, ...] = ()

    @classmethod
    def from_record(cls, record: dict[str, Any], clean: bool = True) -> Context:
        data = record["input"]
        if record["suite"] == "recipes":
            history = "\n".join(entry["command"] for entry in data["history"])
            if not clean:
                return cls("recipes", history)
            cleaned = sanitize.redact(history)
            return cls("recipes", cleaned.text, cleaned.removed)
        if not clean:
            return cls("translate", data["screen"])
        cleaned = sanitize.clean(data["screen"])
        return cls("translate", cleaned.text, cleaned.removed)

    @property
    def last_line(self) -> str:
        lines = [line.strip() for line in self.text.split("\n") if line.strip()]
        return lines[-1] if lines else ""


class Rejected(ValueError):
    pass


def _types(schema: dict[str, Any]) -> list[str]:
    kind = schema.get("type", "string")
    return kind if isinstance(kind, list) else [kind]


def _matches(value: Any, kind: str) -> bool:
    if kind == "null":
        return value is None
    if kind == "boolean":
        return isinstance(value, bool)
    if kind == "integer":
        return isinstance(value, int) and not isinstance(value, bool) or isinstance(value, float) and value.is_integer()
    if kind == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if kind == "string":
        return isinstance(value, str)
    if kind == "array":
        return isinstance(value, list)
    if kind == "object":
        return isinstance(value, dict)
    return False


def _conform(value: Any, schema: dict[str, Any], name: str) -> Any:
    if not any(_matches(value, kind) for kind in _types(schema)):
        raise Rejected(f"{name} has the wrong type")
    if "enum" in schema and value not in schema["enum"]:
        raise Rejected(f"{name} is not allowed")
    if isinstance(value, dict):
        return _object(value, schema)
    if isinstance(value, list) and "items" in schema:
        return [_conform(item, schema["items"], name) for item in value]
    return value


def _object(args: dict[str, Any], schema: dict[str, Any], optional: frozenset[str] = frozenset()) -> dict[str, Any]:
    properties = schema.get("properties", {})
    for required in [key for key in schema.get("required", []) if key not in optional]:
        if required not in args:
            raise Rejected(f"{required} is missing")
    return {key: _conform(args[key], properties[key], key) for key in properties if key in args}


def _strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [s for item in value.values() for s in _strings(item)]
    if isinstance(value, list):
        return [s for item in value for s in _strings(item)]
    return []


def _on_screen(number: Any, present: list[float]) -> bool:
    return not isinstance(number, (int, float)) or number == 0 or any(canon.close(number, seen) for seen in present)


def _ground(name: str, args: dict[str, Any], context: Context) -> dict[str, Any]:
    text = context.text
    if name == "propose_recipe":
        for param in args.get("params", []):
            default = param.get("default") if isinstance(param, dict) else None
            if default is not None and str(default) not in text:
                raise Rejected("default is not in history")
        return args
    present = canon.numbers_in(text)
    keys = {"progress_updated": ["current", "total"], "metric_reported": ["value"], "server_started": ["port"]}.get(name, [])
    for key in keys:
        value = args.get(key)
        if key == "total" and value == 100 and canon.unit_group(str(args.get("unit", ""))) == "%":
            continue
        if not _on_screen(value, present):
            raise Rejected(f"{key} is not on screen")
    if name == "checkpoint_created":
        args["path"] = args["path"].strip().strip("'\"`")
        if not args["path"] or args["path"] not in text:
            raise Rejected("path is not on screen")
    if name == "server_started" and args.get("url"):
        args["url"] = args["url"].split("?", 1)[0].split("#", 1)[0]
        if args["url"].rstrip("/").lower() not in text.lower():
            raise Rejected("url is not on screen")
    if name == "waiting_for_input" and args["prompt"].strip().lower() not in context.last_line.lower():
        raise Rejected("prompt is not on the last lines")
    return args


def check_call(call: Any, context: Context) -> Call:
    """The call as it may be proposed, or `Rejected` with a short reason."""
    if not isinstance(call, dict) or call.get("name") not in _FUNCTIONS:
        raise Rejected("unknown function")
    name = call["name"]
    if _SUITES.get(name, "translate") != context.suite:
        raise Rejected("not allowed for this input")
    args = call.get("args")
    if not isinstance(args, dict):
        raise Rejected("arguments are not an object")
    args = _object(args, _FUNCTIONS[name]["parameters"], frozenset(NOT_REQUIRED.get(name, ())))
    args = _ground(name, args, context)
    if any(sanitize.has_secret(value, context.removed) for value in _strings(args)):
        raise Rejected("contains a secret")
    summary = args.get("summary")
    if isinstance(summary, str) and len(summary) > SUMMARY_LIMIT:
        raise Rejected("summary is too long")
    return {"name": name, "args": args}


def check_calls(calls: list[Any], context: Context) -> tuple[list[Call], list[tuple[Any, str]]]:
    """(accepted, [(call, reason), ...]); accepted calls keep their order."""
    accepted: list[Call] = []
    rejected: list[tuple[Any, str]] = []
    for call in calls:
        try:
            accepted.append(check_call(call, context))
        except Rejected as reason:
            rejected.append((call, str(reason)))
    return accepted, rejected
