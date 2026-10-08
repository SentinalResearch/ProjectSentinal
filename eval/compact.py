"""The compact prompt format shared by fine-tuning and evaluation.

A fine-tuned model learns the functions from examples, so the prompt carries
one short instruction instead of the full `tools.json` schema (about 5x fewer
tokens). The reply is a JSON array of calls. Training and evaluation must use
exactly these functions, or the model sees a format it was not trained on.
"""

import json
import re

COMPACT_SYSTEM = (
    "Translate the terminal state into Sentinal calls. Reply with a JSON array only, [] when nothing applies.\n"
    "Functions: progress_updated(current,total,unit) latest per unit; metric_reported(name,value) latest per metric; "
    "checkpoint_created(path) files written, not loaded; error_detected(kind) kind in oom,nan,exception,test_failure,"
    "build_error,network,permission,not_found,killed,port_in_use,other; warning_detected() one per warning line; "
    "waiting_for_input(kind,prompt) only if the last line waits, kind in confirm,password,text,key; "
    "server_started(port,url) url without query string; tests_summary(passed,failed,skipped); "
    "propose_recipe(template,params) for history: a command run successfully 3+ times, varying values as {param} "
    "with the latest value as default.\n"
    "Never copy secrets. Report only what is visible now; config files, docs and listings are not events."
)


def render_user(record):
    """The user turn for one dataset record."""
    data = record["input"]
    if record["suite"] == "recipes":
        lines = [f"[exit {entry['exit_code']}] {entry['command']}" for entry in data["history"]]
        return "History:\n" + "\n".join(lines)
    return f"Command: {data['command']}\nSource: {data['source']}\nScreen:\n{data['screen']}"


def render_target(record):
    """The assistant turn a perfect model writes: the expected calls as compact JSON."""
    calls = [{"name": call["name"], "args": call["args"]} for call in record["expected"]]
    return json.dumps(calls, ensure_ascii=False, separators=(",", ":"))


def messages(record, with_target=False):
    turns = [{"role": "system", "content": COMPACT_SYSTEM}, {"role": "user", "content": render_user(record)}]
    if with_target:
        turns.append({"role": "assistant", "content": render_target(record)})
    return turns


_THINK = re.compile(r"<think>.*?</think>", re.DOTALL)


def parse_reply(text):
    """Calls from a model reply; (calls, error). Tolerates fences and stray prose."""
    if not isinstance(text, str):
        return [], "reply has no text"
    body = _THINK.sub("", text).strip()
    body = re.sub(r"^```(?:json)?\s*|\s*```$", "", body).strip()
    start, end = body.find("["), body.rfind("]")
    if start < 0 or end < start:
        return [], "reply has no JSON array"
    try:
        value = json.loads(body[start : end + 1])
    except json.JSONDecodeError:
        return [], "reply JSON is invalid"
    if not isinstance(value, list):
        return [], "reply is not an array"
    calls = [{"name": item.get("name"), "args": item.get("args", {})} for item in value if isinstance(item, dict)]
    return calls, None if len(calls) == len(value) else "reply has non-object items"
