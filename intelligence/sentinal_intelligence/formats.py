"""The prompt formats and function schema the models were trained on.

`eval/` is the single source of the model contract: the dataset, the
function schema (`eval/tools.json`) and the prompt formats used in training.
The service imports them from there, so it always sends a model exactly the
format it learned. (Packaging them separately comes with the agent API.)
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

EVAL = Path(__file__).resolve().parents[2] / "eval"
if str(EVAL) not in sys.path:
    sys.path.insert(0, str(EVAL))

import canon
import compact
import functiongemma

TOOLS_PATH = EVAL / "tools.json"
FORMATS = ("functiongemma", "compact")


def functions() -> dict[str, dict[str, Any]]:
    definitions = json.loads(TOOLS_PATH.read_text(encoding="utf-8"))["functions"]
    return {definition["name"]: definition for definition in definitions}


__all__ = ["FORMATS", "TOOLS_PATH", "canon", "compact", "functiongemma", "functions"]
