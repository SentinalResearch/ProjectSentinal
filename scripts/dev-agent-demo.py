#!/usr/bin/env python3
"""See how an AI agent uses Sentinal, live, against a running `just dev`.

Does what a real agent does, through the same `sentinal` commands and MCP tools:
gets its own token, starts a job with no terminal, follows its progress, notices
when the job needs a person, tries something it may not do, and is revoked.

Keep the dev web page open while it runs: everything appears there too.
`--keep` leaves the token active and prints how to connect Claude Code.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
WIDTH = 72


def say(text: str = "") -> None:
    print(text, flush=True)


def step(number: int, title: str, why: str) -> None:
    say(f"\n{'─' * WIDTH}\n{number}. {title}\n   {why}\n")


def fail(message: str) -> None:
    sys.exit(f"dev-agent: {message}")


def dev_paths() -> dict[str, Path]:
    root = Path(os.environ.get("SENTINAL_DEV_DIR", Path.home() / ".local/share/sentinal-dev"))
    return {"token": root / "config/token", "password": root / "config/dev-password", "kept": root / "config/agent-token"}


def tailscale_ip() -> str:
    explicit = os.environ.get("SENTINAL_TAILSCALE_IP")
    if explicit:
        return explicit
    try:
        return subprocess.run(["tailscale", "ip", "-4"], check=True, capture_output=True, text=True).stdout.split()[0]
    except (OSError, subprocess.CalledProcessError, IndexError):
        fail("cannot read this Machine's Tailscale address; is Tailscale on?")
    raise AssertionError


def http(base: str, method: str, path: str, body: dict | None = None, bearer: str | None = None):
    request = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode())
    request.add_header("Content-Type", "application/json")
    if bearer:
        request.add_header("Authorization", f"Bearer {bearer}")
    with urllib.request.urlopen(request, timeout=10) as reply:
        raw = reply.read()
    return json.loads(raw) if raw else None


class Mcp:
    """`sentinal mcp`, spoken to exactly as an MCP client such as Claude Code does."""

    def __init__(self, binary: Path, control: str, token: str):
        env = {**os.environ, "SENTINAL_AGENT_TOKEN": token}
        self.process = subprocess.Popen(
            [str(binary), "mcp", "--control", control, "--development"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True,
        )
        self.next_id = 0
        self.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dev-agent-demo", "version": "1"},
        })
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message: dict) -> None:
        assert self.process.stdin
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}})
        assert self.process.stdout
        line = self.process.stdout.readline()
        if not line:
            fail("sentinal mcp stopped")
        return json.loads(line)

    def tool(self, name: str, **arguments) -> tuple[dict | str, bool]:
        """(result, refused): what the tool returned, or the words of its refusal."""
        reply = self.request("tools/call", {"name": name, "arguments": arguments})
        if "error" in reply:
            return reply["error"].get("message", "refused"), True
        result = reply["result"]
        text = result["content"][0]["text"]
        if result.get("isError"):
            return text, True
        return json.loads(text), False

    def close(self) -> None:
        if self.process.stdin:
            self.process.stdin.close()
        self.process.wait(timeout=5)


def latest_progress(session: dict) -> str | None:
    progress = [event for event in session.get("events", []) if event.get("facts", {}).get("category") == "PROGRESS"]
    if not progress:
        return None
    facts = max(progress, key=lambda event: event["id"])["facts"]
    return f"{facts['current']}/{facts.get('total')} {str(facts.get('unit', '')).lower()}".strip()


def refusal_words(refusal: object) -> str:
    try:
        return json.loads(str(refusal))["error"]
    except (ValueError, KeyError, TypeError):
        return str(refusal)


def job_script(directory: Path) -> Path:
    # An ordinary program that knows nothing about Sentinal.
    script = directory / "agent-demo-job.sh"
    script.write_text(
        "#!/bin/sh\n"
        "for i in 1 2 3 4 5 6 7 8 9 10; do echo \"task $i/10\"; sleep 2; done\n"
        "echo 'warning: 1 sample was skipped'\n"
        "printf 'Publish the results? [y/N] '\n"
        "read answer\n"
        "echo \"You answered: $answer\"\n"
    )
    script.chmod(0o700)
    return script


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--keep", action="store_true", help="keep the token and print how to connect Claude Code")
    parser.add_argument("--answer-wait", type=int, default=120, help="seconds to wait for you to answer the question")
    args = parser.parse_args()

    paths = dev_paths()
    ip = tailscale_ip()
    daemon_port = os.environ.get("SENTINAL_DEV_DAEMON_PORT", "7520")
    web_port = os.environ.get("SENTINAL_DEV_WEB_PORT", "5273")
    base = f"http://{ip}:{daemon_port}"
    try:
        http(base, "GET", "/api/v1/health")
    except (urllib.error.URLError, TimeoutError):
        fail(f"no dev environment answers at {base}. Start it first in another terminal: just dev")
    if not paths["password"].is_file():
        fail("this dev environment has no saved password. Run: just dev --reset, then just dev")
    version = os.environ.get("SENTINAL_RELEASE_VERSION")
    build_env = dict(os.environ)
    tag = subprocess.run(["git", "-C", str(REPO), "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"],
                         capture_output=True, text=True).stdout.strip()
    if tag and not version:
        build_env["SENTINAL_RELEASE_VERSION"] = tag.removeprefix("v")
        build_env["SENTINAL_GIT_COMMIT"] = subprocess.run(["git", "-C", str(REPO), "rev-parse", "HEAD"],
                                                          capture_output=True, text=True).stdout.strip()
        if subprocess.run(["git", "-C", str(REPO), "status", "--porcelain", "--untracked-files=no"],
                          capture_output=True, text=True).stdout.strip():
            build_env["SENTINAL_GIT_COMMIT"] += "-dirty"
    say("Building this checkout's `sentinal`...")
    subprocess.run(["cargo", "build", "--locked", "-q", "-p", "cli", "--bin", "sentinal"], cwd=REPO, check=True, env=build_env)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    sentinal = target / "debug/sentinal"
    machine_cli = [str(sentinal), "--url", f"ws://{ip}:{daemon_port}/api/v1/ws", "--token-file", str(paths["token"])]
    say(f"Open http://{ip}:{web_port} to watch this happen in the UI.")

    step(1, "The guide an agent reads first", "`sentinal --help` points agents to this, so they need no code from us.")
    guide = subprocess.run([str(sentinal), "agent-guide"], capture_output=True, text=True, check=True).stdout
    say("\n".join(f"   | {line}" for line in guide.splitlines()[:6]))
    say("   | ...")

    step(2, "The owner gives the agent its own token", "Same as Settings → Agents. Level 0 means it may only observe.")
    owner = http(base, "POST", "/api/v1/auth/login", {"password": paths["password"].read_text().strip()})["token"]
    name = f"demo-{int(time.time())}"
    created = http(base, "POST", "/api/v1/agents", {"name": name, "level": "OBSERVE"}, owner)
    agent_id, token = created["agent"]["id"], created["token"]
    say(f"   Agent {name!r} created. Its token is never printed here.")

    mcp = Mcp(sentinal, base, token)
    tools = [tool["name"] for tool in mcp.request("tools/list")["result"]["tools"]]
    say(f"   Tools it can load: {', '.join(tools)}")
    who, _ = mcp.tool("whoami")
    say(f"   whoami → {who['name']}, level {who['level']}")
    machines, _ = mcp.tool("list_machines")
    listed = machines["machines"] if isinstance(machines, dict) else machines
    machine = listed[0]["name"]
    say(f"   list_machines → {', '.join(entry['name'] for entry in listed)}")

    step(3, "The agent starts a job with no terminal", "An unchanged program: it only prints `task 3/10`. Sentinal reads that.")
    with tempfile.TemporaryDirectory(prefix="sentinal-agent-demo-") as directory:
        script = job_script(Path(directory))
        env = {key: value for key, value in os.environ.items() if key != "SENTINAL_SESSION_ID"}
        started = subprocess.run([*machine_cli, "watch", "--detach", "-t", name, "--", str(script)],
                                 capture_output=True, text=True, stdin=subprocess.DEVNULL, env=env)
        if started.returncode != 0:
            fail(f"could not start the job: {started.stderr.strip()}")
        session_id = int(started.stdout.strip())
        say(f"   $ sentinal watch --detach -t {name} -- ./agent-demo-job.sh")
        say(f"   → Session {session_id}. It is on the dashboard now.")

        step(4, "It follows progress through structured Events", "It never reads the screen; terminal output is off for agents.")
        last = None
        state = ""
        session: dict = {}
        deadline = time.time() + 60
        while time.time() < deadline:
            session, missing = mcp.tool("get_session", machine=machine, session_id=session_id)
            if missing:  # Not on the control yet: it arrives within a second.
                time.sleep(0.5)
                continue
            state = session["state"]
            progress = latest_progress(session)
            if progress != last and progress:
                say(f"   get_session → {state}, progress {progress}")
                last = progress
            if state == "WAITING":
                break
            time.sleep(1)
        kinds = sorted({event.get("facts", {}).get("category", "?") for event in session.get("events", [])})
        say(f"   Event types it saw: {', '.join(kinds)}")

        step(5, "The job asks a question, and the agent may not answer", "`wait_for` blocks without polling. A person must decide.")
        if state != "WAITING":
            say(f"   The Session is {state}; Sentinal did not report WAITING within 60 s.")
            result = None
        else:
            say("   The Session is WAITING. Answer it yourself now: open it in the web UI (or phone)")
            say(f"   and press y or n. Waiting up to {args.answer_wait} s...")
            result, _ = mcp.tool("wait_for", machine=machine, session_id=session_id,
                                 until=["RUNNING", "IDLE", "COMPLETED", "FAILED"], timeout_seconds=args.answer_wait)
            answered = isinstance(result, dict) and result.get("matched", False)
            say(f"   wait_for → {'you answered, and the agent carried on' if answered else 'no answer in time; an agent would tell you and stop'}")

        step(6, "It tries to type into the terminal", "Sentinal offers an agent no such tool at all, whatever the model tries.")
        refusal, refused = mcp.tool("send_input", machine=machine, session_id=session_id, text="y\n")
        say(f"   send_input → {'refused: ' + refusal_words(refusal) if refused else 'NOT REFUSED: this is a bug'}")

        if args.keep:
            step(7, "Token kept, so you can connect Claude Code", "Saved in a private file inside the dev folder.")
            mcp.close()
            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            with os.fdopen(os.open(paths["kept"], flags, 0o600), "w") as file:
                file.write(token + "\n")
            say(f"   claude mcp add sentinal-dev -- {sentinal} mcp --control {base} \\")
            say(f"     --development --token-file {paths['kept']}")
            say(f"\n   Then ask: \"What is running in dev, and does anything need me?\"")
            say("   Remove it later: claude mcp remove sentinal-dev; revoke it in Settings → Agents.")
            refused_after = True
        else:
            step(7, "The owner revokes the token", "It stops working on the very next request.")
            http(base, "DELETE", f"/api/v1/agents/{agent_id}", None, owner)
            after, refused_after = mcp.tool("whoami")
            say(f"   whoami → {'refused: ' + refusal_words(after) if refused_after else 'STILL WORKS: this is a bug'}")
            mcp.close()

    say(f"\n{'─' * WIDTH}")
    say(f"Done. Session {session_id} ({name}) stays in dev for you to open, hide, or delete.")
    if not args.keep:
        say("Run with --keep to leave the agent active and see its calls under Settings → Agents → Activity.")
    return 0 if refused and refused_after else 1


if __name__ == "__main__":
    raise SystemExit(main())
