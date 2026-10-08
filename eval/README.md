# Translator evaluation

A scored benchmark for models (or code) that turn a terminal screen into
Sentinal protocol calls, plus a Recipe-proposal suite. Standard library Python
only; the same seed always produces the same data.

```sh
python3 eval/generate.py                      # eval/data/test.jsonl, dev.jsonl, manifest.json
python3 eval/generate.py --train 5000         # also train.jsonl for fine-tuning (not committed)
python3 -m unittest discover -s eval -p 'test_*.py'

# Today's deterministic adapters as the baseline to beat
cargo run -q -p watcher --example eval_baseline -- eval/data/test.jsonl > baseline.jsonl
python3 eval/score.py eval/data/test.jsonl baseline.jsonl --edges
```

## Running an OpenAI-compatible model

`run_model.py` calls a vLLM, llama.cpp, or other OpenAI-compatible server and
writes the prediction JSONL consumed by `score.py`. Start with `dev.jsonl` and
change prompts or inference settings only against that split:

```sh
python3 eval/run_model.py eval/data/dev.jsonl \
  --base-url http://POP_OS_TAILSCALE_IP:8343/ \
  --model Qwen/Qwen3-4B \
  --output qwen3-4b-dev.jsonl \
  --concurrency 4

python3 eval/score.py eval/data/dev.jsonl qwen3-4b-dev.jsonl \
  --edges \
  --json qwen3-4b-dev-report.json \
  --failures qwen3-4b-dev-failures.jsonl
```

A bare `IP:port`, server root, or `/v1` URL is accepted. When the server
advertises exactly one model, `--model` may be omitted. vLLM normally serves
one loaded model per process, so restart it with the next model and use a
different output file for each comparison. `--limit 3` provides a cheap smoke
test, while `--resume` keeps completed predictions after an interrupted run.

For an authenticated server, place the key in an owner-only file rather than
the command line:

```sh
python3 eval/run_model.py eval/data/dev.jsonl \
  --base-url http://POP_OS_TAILSCALE_IP:8343/ \
  --api-key-file /tmp/sentinal-qwen-api-key \
  --output qwen-dev.jsonl
```

The runner sends only each record's `suite` and `input` fields. It never sends
`expected`, `optional`, or `forbidden` labels to the model. A prediction is
written and flushed after every response, including transport and parsing
failures, so the scorer can distinguish a failed response from a missing
example.

Every prediction also records the reply's `raw` text (first 2,000 characters),
`finish_reason`, and token counts. A model that "returns zero calls" can then be
told apart from one whose output was never parsed. In the 2026-10-04 run, the
270M models were served with the Hermes tool parser, and Gemma 3's chat
template dropped the tools, so their zeros measured the setup, not the models.

## Fine-tuning and the compact format

A fine-tuned model learns the functions from examples, so it doesn't need the
full `tools.json` schema in every prompt. [`compact.py`](compact.py) defines a
short instruction and a JSON-array reply, about 5× fewer tokens. Training and
evaluation import the same module, so the model is always tested in the format
it learned.

```sh
python3 eval/generate.py --train 5000
# on the GPU machine (torch, transformers, trl, datasets)
python3 eval/finetune/train_sft.py --model Qwen/Qwen3-0.6B \
  --train eval/data/train.jsonl --dev eval/data/dev.jsonl --output runs/qwen3-0.6b-sft
vllm serve runs/qwen3-0.6b-sft --served-model-name sentinal-qwen3-0.6b --dtype bfloat16 --port 8345
python3 eval/run_model.py eval/data/dev.jsonl --format compact --concurrency 16 \
  --base-url http://127.0.0.1:8345 --model sentinal-qwen3-0.6b --output sft-dev.jsonl
```

The training script:
- renders prompts with the model's own chat template, with thinking disabled,
  exactly as vLLM serves them;
- computes loss on the answer only;
- refuses any record from the test split.

Choose settings on `dev`, then run `test` once.

**Measured 2026-10-04** (details in
[`docs/progress/v2-translator-eval.md`](../docs/progress/v2-translator-eval.md)):

| System | Split | F1 | Secret leaks | p50 latency |
|---|---|---|---|---|
| Deterministic rules | test | 0.317 | 0 | — |
| Qwen3-4B, prompted | dev | 0.561 | 2 | 1,454 ms |
| Qwen3-0.6B, fine-tuned (42.6 min, one RTX 5060 Ti) | test | 0.978 | 0 | 293 ms |

The fine-tuned model scores 0.909 on held-out formats and 0.914 on the 8 real
logs. Train and test share one generator, so a hand-labelled set of real
captured screens is the next decision metric.

## The task

**Translate.** The input is the command and the current screen (last 40 lines).
For `source: "log"` it is the raw tail of a log file instead, with colour codes
and carriage returns left in. The output is calls from [`tools.json`](tools.json)
describing what the screen shows *now*:

- the latest progress per unit;
- the latest value per metric;
- every checkpoint written;
- each distinct error kind;
- each warning;
- the newest server and test summary;
- a prompt only when it is the last line.

Nothing else. A config file listing `epochs: 10` is not progress, and `Loading
checkpoint` is not a checkpoint.

**Recipes.** The input is a command history. A command shape that succeeded at
least 3 times becomes a `propose_recipe`:

- values that changed become `{params}`, defaulting to the latest value;
- values that never changed stay literal;
- failed runs don't count;
- secrets never appear.

`metric_reported`, `tests_summary`, and the `kind` of errors and prompts are
proposed additions to the protocol, marked as such in `tools.json`.

## Data

| Category | What it covers |
|---|---|
| training | 14 log formats: plain, `N of M`, logfmt, JSON lines, HF dicts + bars, tqdm, Keras, timestamped, rank-prefixed, percent-only, W&B-style, RL, non-English, Lightning bars |
| errors | OOM, exceptions, network, permission, not found, killed or segfault, build errors (cargo, tsc, npm, make), port in use |
| prompts | confirm, password, text, press-a-key, and prompts already answered (nothing waits) |
| servers | uvicorn, vLLM, llama.cpp, Vite, Next.js, Flask, Gradio, Django, Go, Jupyter (token must be stripped) |
| tests | pytest, cargo, jest, vitest, unittest (derived pass count), go |
| downloads | Hugging Face nested bars, pip, git clone (two progress forms), wget |
| negatives | `ls`, `git log`, `nvidia-smi`, config files, `ping`, `docker ps`, `df`, READMEs, `history`, `top`: nothing to report |
| real | the 8 captured logs in `crates/watcher/tests/logs`, labelled by hand |
| recipes | histories with 0–3 repeated command families, failed runs, constant values, noise, secrets |

The data also includes these edge cases:
- distractor numbers: versions, seeds, ports, paths like `epoch=2-val_loss=0.55.ckpt`;
- secrets in about 12% of screens (scored as leaks if echoed);
- colour codes;
- scrolled-off facts;
- crashes in the middle of training;
- unusual launchers (`bash run.sh`, `deepspeed`, `uv run`).

`manifest.json` lists the counts per category, function, and edge case.

**Splits.**
- `test` holds back four formats (W&B, RL, non-English, Lightning bars) that
  `dev` and `train` never contain, so "held-out formats" measures
  generalisation.
- `train` never contains a `test` input.
- Inputs within each split are unique.

**Optional calls.** Some readings are honest but not required, such as Hugging
Face's fractional `'epoch': 1.80`, a finished run's `224/224 tests`, or a test
failure error beside its summary. A model gains nothing for them and loses
nothing for them.

## Scoring

The scorer pairs each predicted call with an expected one. A match allows for:
- metric and unit synonyms (`lr` = `learning_rate`, `it` = `step`);
- a percentage reading of the same progress (`27 %` for `270/1000`);
- printing precision.

It reports:

- **F1, precision, recall** by function, category, seen versus held-out
  formats, and edge case (`--edges`, worst first).
- **Exact match:** examples with no miss and no extra call.
- **Invented values:** predicted numbers or paths that are not in the input.
- **Secret leaks:** examples where any output contains a planted secret. This
  must be 0.
- **Clean negatives:** screens with nothing to report that got no calls.
- **Errors spotted (any kind):** separates "noticed" from "classified".
- Invalid calls, missing predictions, Recipe defaults, latency (from
  `latency_ms`).

## Plugging in a model

Write one JSON line per example:

```json
{"id": "test-train-plain-0023", "calls": [{"name": "progress_updated", "args": {"current": 2, "total": 20, "unit": "epoch"}}], "latency_ms": 41}
```

Give the model the functions from `tools.json` and the example's `input`,
parse its function calls, and score the file. Invalid calls count against
precision, so a model that writes malformed calls is penalised.
