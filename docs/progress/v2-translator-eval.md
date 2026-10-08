# V2 exploration — screen translator evaluation and first fine-tune

Status: **exploratory, 2026-10-04.** No model ships in the product. This
records the evaluation harness, the model comparison, and the first
fine-tuned translator so later work starts from measured numbers.

## Task

Given a command and its current screen (or log tail), emit the Sentinal calls
it implies, as defined in [`eval/tools.json`](../../eval/tools.json):
- progress;
- metrics;
- checkpoints;
- errors by kind;
- warnings;
- prompts;
- servers;
- test summaries;
- Recipe proposals from command history.

[`eval/README.md`](../../eval/README.md) defines the task and the scoring.

## Data

`python3 eval/generate.py --train 5000` (seed `sentinal-eval-v1`, dataset version 1).

| Split | Examples | Expected calls | Notes |
|---|---|---|---|
| test | 1,528 | 3,214 | Includes 4 held-out log formats (W&B-style, RL, non-English, Lightning bars) and the 8 hand-labelled real logs |
| dev | 286 | 541 | No held-out formats |
| train | 4,040 | 7,636 | Never contains a test input (checked at generation) |

## Hosts

- **Evaluation and training:** pop-os, Linux (Pop!_OS 24.04).
  - 2× NVIDIA RTX 5060 Ti (16 GB + 8 GB). Training used only the 16 GB card.
  - PyTorch 2.11.0+cu128, transformers 5.13.1, TRL 1.14.1, vLLM 0.25.1.
- **Scoring:** macOS (Mac mini) and pop-os, with the same `eval/score.py`.

## Results

| System | Split | F1 | Precision | Recall | Exact | Invented values | Secret leaks | Clean empty screens | Latency p50 / p95 |
|---|---|---|---|---|---|---|---|---|---|
| Deterministic rules (`watcher` adapters) | test | 0.317 | 0.661 | 0.209 | 22.8% | 3.1% | 0 | 95.9% | — |
| Deterministic rules | dev | 0.332 | 0.644 | 0.224 | 25.9% | 3.0% | 0 | 100% | — |
| Qwen3-4B, prompted, tool calls | dev | 0.561 | 0.606 | 0.523 | 40.6% | 4.8% | 2 | 70.9% | 1,454 / 4,910 ms |
| Qwen3-4B + code guards (post-hoc) | dev | 0.568 | 0.622 | 0.523 | 41.6% | 1.0% | 0 | 74.6% | — |
| Qwen3-1.7B, prompted | dev | 0.210 | 0.233 | 0.192 | 7.7% | 13.6% | 4 | 0% | 708 / 2,789 ms |
| Qwen3-0.6B, prompted | dev | 0.143 | 0.154 | 0.133 | 14.0% | 12.4% | 0 | 20% | 526 / 2,563 ms |
| Qwen3-0.6B, untrained, compact prompt | dev | 0.000 | — | 0.000 | 18.9% | 0% | 0 | 96.4% | — |
| **Qwen3-0.6B fine-tuned (`sentinal-qwen3-0.6b`)** | dev | **0.989** | 0.993 | 0.985 | 97.2% | 0.2% | 0 | 100% | — |
| **Qwen3-0.6B fine-tuned** | **test** | **0.978** | 0.986 | 0.970 | 96.0% | 0.9% | **0** | 99.7% | **293 / 894 ms** |

The test run of the fine-tuned model broken down further:

| Slice | F1 |
|---|---|
| Seen formats | 0.998 |
| **Held-out formats** | **0.909** |
| **8 real logs** | **0.914** |
| Weakest edge case: Lightning bars | 0.736 |

The whole test split ran in 38 s at concurrency 16. There was 1 malformed reply
in 1,528.

### The 270M–360M models were not measured

FunctionGemma 270M, Gemma 3 270M and SmolLM2 360M returned zero calls on all
dev examples. The cause was the serving setup, not the models:

1. vLLM served every model with `--tool-call-parser hermes`.
2. Gemma 3's chat template drops the tool list, so the prompt was 398 tokens
   with no functions in it, and the model replied with one empty token.

With the functions written into the prompt, Gemma 3 270M answered in the
right format but repeated the prompt's example for every screen. These three
models need fine-tuning, or the correct parser, before they can be compared.
`run_model.py` now records each reply's raw text, stop reason, and token
counts, so a silent zero can be explained.

## Fine-tune

```sh
python3 eval/finetune/train_sft.py --model Qwen/Qwen3-0.6B \
  --train eval/data/train.jsonl --dev eval/data/dev.jsonl \
  --output runs/qwen3-0.6b-sft --epochs 3
# then, with the compact prompt the model was trained on
python3 eval/run_model.py eval/data/test.jsonl --format compact --concurrency 16 \
  --base-url http://127.0.0.1:8345 --model sentinal-qwen3-0.6b --output sft-test.jsonl
```

Full fine-tune settings:
- 3 epochs, learning rate 2e-5, cosine schedule, effective batch 16;
- bf16 mixed precision with gradient checkpointing;
- loss on the answer only;
- 1.67 M tokens per epoch.

Training took **42.6 minutes** on the 16 GB card, using about 14 GB.

Dev loss per epoch was 0.0200, 0.0071 and 0.0077, so two epochs would have
been enough. The prompt is rendered with the model's own chat template exactly
as vLLM serves it, with thinking disabled ([`eval/compact.py`](../../eval/compact.py)).
The model was served in bf16 with vLLM.

## Limitations

- **Mostly synthetic.** Train and test come from the same generator, so the
  seen-format score is close to template memorisation. The held-out formats
  (0.909) and the 8 real logs (0.914) are the honest numbers, and real-world
  accuracy is unproven.
- **Number transcription error on a real log.** On the real Hugging Face log
  the model read `5e-05` as `0.0005`, ten times too large. Its on-screen check
  would drop that value rather than correct it.
- **Systematic held-out miss.** Lightning bars lose the "Epoch N" counter that
  has no total; all 44 Lightning failures are this pattern.
- **Small dev set.** 286 examples, so differences under about 0.03–0.05 F1 are
  noise.

## Next

1. **Hand-label 100–200 real screens** captured by Sentinal on the three
   Machines. This becomes the decision metric.
2. **Code guards:**
   - JSON-constrained decoding;
   - on-screen grounding;
   - URL and secret cleaning;
   - number-notation sanity checks.
3. **Broaden the generator** (exponent styles, counters without a total,
   noise), then retrain. A run takes about 45 min.
4. **Re-measure the 270M models** with the correct serving setup, or by
   fine-tuning them.

## Artifacts (not committed)

- The fine-tuned model is on pop-os at `~/sentinal-train/runs/qwen3-0.6b-sft`
  (2.4 GB, fp32).
- Predictions and reports are in `tmp/sentinal-eval/` on the control Mac.
