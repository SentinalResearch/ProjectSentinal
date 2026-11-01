"""Call a local OpenAI-compatible model server (vLLM or llama.cpp) in a trained format.

The request and reply parsing are the evaluation runner's own
(`eval/run_model.py`), so the service talks to a model exactly the way it
was measured.
"""

from __future__ import annotations

from typing import Any
from urllib.request import urlopen

import run_model

from . import formats


def model_client(base_url: str, model: str, prompt_format: str = "functiongemma", api_key: str | None = None,
                 timeout: float = 10.0, opener: Any = urlopen):
    """A callable record -> {"calls", "error"?, "latency_ms"} bound to one server and format."""
    if prompt_format not in formats.FORMATS:
        raise ValueError(f"unknown format {prompt_format!r}")
    url = run_model.normalize_base_url(base_url)

    def call(record: dict[str, Any]) -> dict[str, Any]:
        return run_model.evaluate_record(record, url, api_key, model, [], timeout, opener=opener, prompt_format=prompt_format)

    return call
