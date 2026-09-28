# MILESTONE 7 — Fine-tuned small model + local agent (V2 part 3)

Goal: a sub-1B model that routes requests and triages state reliably,
plus the heartbeat, journal and summaries built mostly without it.

## 1. Fine-tuning pipeline (`intelligence/train/`)

- Start with the task chosen in Milestone 6 (expected: `route` with
  FunctionGemma-270M; `classify`/`triage` with Qwen3-0.6B).
- TRL `SFTTrainer`; full fine-tune for ≤ 0.6B on the RTX GPU, LoRA as an option.
  Config in YAML; seed fixed; train/dev split only, `test` untouched.
- Chat template must match the base model's official template (verify with
  a test that renders one example and compares to the tokenizer's own
  `apply_chat_template`).
- Export: merge → GGUF (llama.cpp `convert_hf_to_gguf.py`) → quantize
  (Q8_0 and Q4_K_M). Record sizes and tokens/sec.
- **Promotion gate:** a model is promoted only if on the frozen `test` split
  it beats the prompted baseline and meets: `route` exact-function ≥ 95 %,
  JSON validity 100 % with grammar; `triage` NOTIFY precision ≥ 0.9;
  injection set: 0 unsafe outputs. Otherwise report and stop.
- Everything reproducible from one command: `just train route`.

## 2. Local agent (`intelligence/agent/`)

- **Heartbeat** every 3 min (configurable): fetch state diff via tool API →
  deterministic pre-filter (nothing changed → `NO_ACTION` with no model call)
  → model triage → if `NOTIFY`, send via `NOTIFY` tool.
- **Summaries:** template-first from structured state
  ("Training reached epoch 68/100 and remains healthy. Validation loss
  improved from 0.41 to 0.37."). Model only rephrases or is skipped.
- **Journal:** per project per day, compact lines from events
  (`10:02 checkpoint saved`), stored in SQLite; "What happened while I was
  gone?" = journal since last seen, templated.
- **Natural-language box** in the web UI: request → `route` model → tool
  call shown to the user → executed only if it is a read or a suggestion.
- **Suggestions (rules first):** CUDA OOM → compare with the last successful
  run of the same recipe/command and suggest the differing argument
  (e.g. batch size). Shown with "Open in terminal"; never executed.
- Optional: a larger model on the RTX PC for open-ended "What's going on
  with TIM?", behind a config flag, reading only structured state.

## Tests first

- Heartbeat: no-change tick makes zero model calls (mock LLM counts calls).
- Summaries: templated outputs for fixed states (golden strings).
- Journal: event stream → expected lines.
- Suggestion rule: two fixture commands differing in `--batch-size` →
  exact suggested command, shell-quoted.
- Agent cannot execute: end-to-end test with a malicious log line in state;
  assert no input was sent to any session and an audit entry exists if it tried.

## Done when

Promotion gate passed (or clearly reported as not passed), agent runs
against a real daemon for 24 h without false-alarm notifications you'd
reject, `just check` passes. Final report, stop.
