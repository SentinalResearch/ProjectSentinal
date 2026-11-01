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

    serve = commands.add_parser("serve", help="translate the Sessions the owner allows and propose Events (6.2)")
    serve.add_argument("--control", default=os.environ.get("SENTINAL_CONTROL_URL"), required="SENTINAL_CONTROL_URL" not in os.environ,
                       help="the control origin, e.g. https://sentinal.<tailnet>.ts.net")
    serve.add_argument("--token-file", type=Path, help="agent token file (else SENTINAL_AGENT_TOKEN); the agent needs level 1 and output access")
    serve.add_argument("--base-url", default=os.environ.get("SENTINAL_MODEL_URL"), required="SENTINAL_MODEL_URL" not in os.environ,
                       help="the local model server")
    serve.add_argument("--model", default=os.environ.get("SENTINAL_MODEL", "sentinal-functiongemma"))
    serve.add_argument("--format", choices=formats.FORMATS, default="functiongemma")
    serve.add_argument("--timeout", type=float, default=10.0)
    serve.add_argument("--interval", type=float, default=10.0, help="seconds between passes (and per-Session model calls)")
    return root


def _default_model(args: argparse.Namespace):
    return llm.model_client(args.base_url, args.model, args.format, os.environ.get("SENTINAL_MODEL_API_KEY"), args.timeout)


def _serve(args: argparse.Namespace) -> int:
    from .control import AgentApi
    from .serve import serve_forever

    token = args.token_file.read_text(encoding="utf-8").strip() if args.token_file else os.environ.get("SENTINAL_AGENT_TOKEN", "")
    api = AgentApi(args.control, token)
    serve_forever(api, Translator(_default_model(args), min_interval=args.interval), args.interval)
    return 0


def main(argv: list[str] | None = None, model_factory=_default_model) -> int:
    args = parser().parse_args(argv)
    if args.command_name == "serve":
        return _serve(args)
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
