"""Command line: try the translator on a saved screen.

    python3 -m sentinal_intelligence translate --command "python train.py" \
        --screen-file screen.txt --base-url http://127.0.0.1:8346 --model sentinal-functiongemma

Prints the proposed Events and the rejected calls as JSON. It talks only to
the model server: nothing is sent to a daemon or a Session.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from . import formats, llm
from .translator import Translator


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="sentinal_intelligence", description=__doc__,
                                   formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = root.add_subparsers(dest="command_name", required=True)
    translate = commands.add_parser("translate", help="propose Events for one saved screen")
    translate.add_argument("--command", required=True, help="the command running in the Session")
    translate.add_argument("--screen-file", required=True, type=Path, help="the screen or log tail, '-' for stdin")
    translate.add_argument("--base-url", default=os.environ.get("SENTINAL_MODEL_URL"), required="SENTINAL_MODEL_URL" not in os.environ)
    translate.add_argument("--model", default=os.environ.get("SENTINAL_MODEL", "sentinal-functiongemma"))
    translate.add_argument("--format", choices=formats.FORMATS, default="functiongemma")
    translate.add_argument("--timeout", type=float, default=10.0)
    return root


def _default_model(args: argparse.Namespace):
    return llm.model_client(args.base_url, args.model, args.format, os.environ.get("SENTINAL_MODEL_API_KEY"), args.timeout)


def main(argv: list[str] | None = None, model_factory=_default_model) -> int:
    args = parser().parse_args(argv)
    screen = sys.stdin.read() if str(args.screen_file) == "-" else args.screen_file.read_text(encoding="utf-8", errors="replace")
    proposal = Translator(model_factory(args), min_interval=0).propose("cli", args.command, screen)
    if proposal is None:
        print(json.dumps({"events": [], "rejected": [], "note": "blank screen; the model was not called"}))
        return 0
    print(json.dumps({
        "events": proposal.events,
        "rejected": [{"call": call, "reason": reason} for call, reason in proposal.rejected],
        "error": proposal.error,
        "latency_ms": proposal.latency_ms,
    }, ensure_ascii=False, indent=2))
    return 1 if proposal.error else 0


if __name__ == "__main__":
    raise SystemExit(main())
