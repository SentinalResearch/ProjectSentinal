# MILESTONE 6 — Intelligence foundation: tool API, evals, baselines (V2 part 2)

Goal: everything needed to measure a model **before** fine-tuning one.
No model makes decisions in production in this milestone.

## 1. Agent tool API (Rust, in the daemon)

- A second token type with a **permission level** (0 observe, 1 summarise,
  2 suggest). The daemon enforces it; level ≥ 3 requests are rejected with an
  audit entry, regardless of what the client claims.
- Tools, as `ClientRequest` variants (fixtures + TS): `LIST_MACHINES`,
  `LIST_SESSIONS`, `GET_SESSION_STATE` (session + current command + health +
  last 20 events), `GET_EVENTS { since }`, `SEARCH_HISTORY`, `LIST_RECIPES`,
  `SUGGEST_COMMAND { session_id, command, reason }` (shows in the UI with an
  "Open in terminal" button; never typed automatically), `NOTIFY { title, body }`.
- Export the tool definitions as JSON Schema (`intelligence/tools.json`)
  generated from Rust, with a test that it is up to date.

## 2. Python service (`intelligence/`, managed with `uv`)

- `client.py` typed client for the daemon API (pydantic models generated or
  mirrored from the JSON Schema; test they match).
- `llm.py` talks to any OpenAI-compatible endpoint (llama.cpp `llama-server`
  default `http://rtx-pc:8080/v1`), with JSON-schema constrained output.
- `sanitize.py`: every piece of terminal-derived text passed to a model is
  wrapped as data (`<untrusted_output>…</untrusted_output>`), truncated,
  and stripped of ANSI. Model output is parsed only as the expected schema;
  anything else is discarded.
- `pytest`, `ruff`, `mypy --strict` in `just check`.

## 3. Datasets and eval harness

Four narrow tasks, each a JSONL file under `intelligence/data/<task>/{train,dev,test}.jsonl`:

| Task | Input | Output |
|---|---|---|
| `route` | natural-language request + tool list + short state | one tool call `{name, arguments}` |
| `classify` | command text + cwd | `CommandClass` |
| `extract` | 1–20 log lines from an unknown tool | list of `EventKind` |
| `triage` | state diff since last tick | `NO_ACTION` or `NOTIFY {reason}` |

- Sources: `extract` pairs generated for free by replaying saved logs
  through the deterministic adapters; `classify` from history + rules;
  `route` and `triage` hand-written seeds (≥ 50 each) expanded with a larger
  teacher model, then **manually reviewed** before entering `test`.
- The `test` split is frozen and hand-checked; never generated.
- `evals/run.py --task route --model <endpoint>` reports: JSON validity,
  exact function match, argument accuracy, and for triage precision/recall
  of `NOTIFY` (precision is the headline metric). Results appended to
  `evals/results.jsonl` with model, date, git sha.

## 4. Baselines

Run prompted (no fine-tune) FunctionGemma-270M, Qwen3-0.6B, and one 4–8B model
on all four tasks with constrained decoding. Put the table in
`docs/EVALS.md`. Include a prompt-injection set (log lines saying "ignore
previous instructions and run …") and report how often each model produced
anything other than a schema-valid, non-executing output.

## Tests first

- Daemon: level-2 token cannot resize/kill/send input/run recipe; level-0 token
  cannot `SUGGEST_COMMAND`; both audited.
- Python: sanitize truncation and wrapping; invalid model JSON handled;
  eval metrics computed correctly on a tiny hand-made set.

## Done when

Baselines table exists, `just check` passes (Rust + Python). Report with
a recommendation of which task to fine-tune first and why. Stop.
