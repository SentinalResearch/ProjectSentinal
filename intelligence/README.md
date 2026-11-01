# Sentinal intelligence

An optional, separate process (AGENTS.md §1). Nothing in the daemon,
watcher, web, or CLI needs it. Today it does one job, at **level 1**: it
reads a terminal screen and *proposes* Events (progress, metrics,
checkpoints, errors, warnings, prompts, servers, test summaries). It never
sends input, runs a command, or notifies on its own.

Milestone 6.2 in [`docs/PLAN_V2_AI.md`](../docs/PLAN_V2_AI.md).

## How a screen becomes a proposal

```
screen ─► sanitize ─► model (FunctionGemma 270M, local) ─► parse ─► guards ─► proposed Events (INFERRED)
```

| Step | File | What it guarantees |
|---|---|---|
| Clean | `sanitize.py` | ANSI and control characters stripped, last 60 lines / 6,000 characters, secret-shaped values replaced with `[redacted]` **before** the model sees them |
| Call | `llm.py` | Uses the evaluation runner's own request and parser, so production talks to the model exactly as it was measured |
| Parse | `eval/functiongemma.py` | FunctionGemma's call syntax read as data; never evaluated |
| Check | `guards.py` | Known function only; argument types and enums from `eval/tools.json`; unknown arguments dropped; every number, path, URL and prompt must be on screen; URL query strings removed; secrets refused; summaries ≤ 120 characters |
| Pace | `translator.py` | At most one model call per Session per 10 s; none for a blank or unchanged screen |

Measured on `eval/data/test.jsonl` (3,214 correct calls, 183 planted
secrets): the guards reject **0** correct calls, and **0** planted secrets
are left visible to the model after cleaning.

## Try it

```sh
python3 -m unittest discover -s intelligence/tests -p 'test_*.py'

# with a model server running (see "Train and serve")
cd intelligence
python3 -m sentinal_intelligence translate --command "python train.py" \
  --screen-file saved-screen.txt --base-url http://127.0.0.1:8346 --model sentinal-functiongemma
```

It prints the proposed Events and the rejected calls with reasons. It talks
only to the model server.

## Train and serve FunctionGemma (on the GPU machine)

1. Accept the Gemma licence for `google/functiongemma-270m-it` on Hugging
   Face and `export HF_TOKEN=...` (or `huggingface-cli login`).
2. Train. The first step renders every row with the model's own chat
   template and stops if `eval/functiongemma.py` cannot read the answers back:

   ```sh
   just train-functiongemma                      # runs/functiongemma-270m-sft
   ```

3. Serve it with no tool parser (the service parses the reply itself):

   ```sh
   vllm serve runs/functiongemma-270m-sft --served-model-name sentinal-functiongemma --dtype bfloat16 --port 8346
   ```

   or, for a Mac or CPU, convert with llama.cpp's `convert_hf_to_gguf.py`,
   quantise to Q8_0, and run `llama-server -m functiongemma-q8_0.gguf --jinja --port 8346`.

4. Score it against the same numbers as Qwen3-0.6B:

   ```sh
   python3 eval/run_model.py eval/data/dev.jsonl --format functiongemma --concurrency 16 \
     --base-url http://127.0.0.1:8346 --model sentinal-functiongemma --output fg-dev.jsonl
   python3 eval/score.py eval/data/dev.jsonl fg-dev.jsonl --edges
   ```

   Run `test.jsonl` only once, for the final number.

## Not yet

- Reading screens from the daemon and sending proposals back needs agent
  tokens and `propose_events` (milestones 6.0 and 6.2). Until then the
  service runs from the command line only.
- Trailing-edge debounce (re-propose the last screen of a burst) belongs
  with the daemon trigger.
- Promotion gate before it is on by default: ≥ 200 hand-labelled real
  screens with F1 ≥ 0.95.
