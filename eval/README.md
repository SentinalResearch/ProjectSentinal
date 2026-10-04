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
