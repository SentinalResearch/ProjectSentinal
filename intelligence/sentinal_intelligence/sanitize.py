"""Clean terminal text before a model sees it.

Terminal output is untrusted (AGENTS.md §5). Before a screen reaches a
model it loses its escape sequences, is cut to the last lines, and has
secret-shaped values replaced, so a model cannot leak what it never saw.
The removed values are kept (in memory only) so the guards can refuse any
reply that still contains one.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

REDACTED = "[redacted]"

_ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]")
_CONTROL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f]")

# Whole-match secret shapes.
SECRET_SHAPES = [
    re.compile(r"\bhf_[A-Za-z0-9]{20,}"),
    re.compile(r"\bsk-[A-Za-z0-9_-]{20,}"),
    re.compile(r"\bAKIA[A-Z0-9]{16}\b"),
    re.compile(r"\bgh[pousr]_[A-Za-z0-9]{20,}"),
    re.compile(r"\bglpat-[A-Za-z0-9_-]{20,}"),
    re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
]

# The secret is group 2; group 1 is kept.
_SECRET_CONTEXTS = [
    re.compile(r"(?i)(\bBearer\s+)([A-Za-z0-9._~+/=-]{8,})"),
    re.compile(r"(://[^/\s:@]+:)([^/\s@]+)(?=@)"),
    re.compile(r"(?i)([?&](?:token|key|api_key|apikey|access_token|auth|sig|signature|password|secret)=)([^&\s#]+)"),
    re.compile(r"(?i)(\bnetrc file:\s*)(\S+)"),
    re.compile(r"(?i)(\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|ACCESS_KEY)[A-Z0-9_]*\s*[=:]\s*)([^\s'\"]+)"),
]


@dataclass(frozen=True)
class Cleaned:
    text: str
    removed: tuple[str, ...] = field(default=())


def strip_ansi(text: str) -> str:
    text = _ANSI.sub("", text).replace("\r\n", "\n")
    # A bare carriage return redraws the line (progress bars): keep what was drawn last.
    text = "\n".join(line.rsplit("\r", 1)[-1] for line in text.split("\n"))
    return _CONTROL.sub("", text)


def redact(text: str) -> Cleaned:
    removed: list[str] = []

    def whole(match: re.Match[str]) -> str:
        removed.append(match.group(0))
        return REDACTED

    def context(match: re.Match[str]) -> str:
        value = match.group(2)
        if REDACTED in value:
            return match.group(0)
        removed.append(value)
        return match.group(1) + REDACTED

    for pattern in SECRET_SHAPES:
        text = pattern.sub(whole, text)
    for pattern in _SECRET_CONTEXTS:
        text = pattern.sub(context, text)
    return Cleaned(text, tuple(dict.fromkeys(removed)))


def clean(raw: str, max_lines: int = 60, max_chars: int = 6000) -> Cleaned:
    """The last `max_lines` lines (at most `max_chars` characters) with ANSI and secrets removed."""
    lines = strip_ansi(raw).split("\n")
    text = "\n".join(lines[-max_lines:])[-max_chars:]
    return redact(text)


def has_secret(value: str, removed: tuple[str, ...] = ()) -> bool:
    if any(secret and secret in value for secret in removed):
        return True
    if any(pattern.search(value) for pattern in SECRET_SHAPES):
        return True
    return any(pattern.search(value) and REDACTED not in value for pattern in _SECRET_CONTEXTS[:1])
