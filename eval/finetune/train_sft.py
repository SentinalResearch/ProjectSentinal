#!/usr/bin/env python3
"""Supervised fine-tuning of a small model on the Sentinal translator dataset.

Runs on the GPU machine (needs torch, transformers, trl, datasets):

    python3 eval/finetune/train_sft.py --model Qwen/Qwen3-0.6B \\
        --train eval/data/train.jsonl --dev eval/data/dev.jsonl --output runs/qwen3-0.6b-sft

The prompt is rendered with the model's own chat template exactly as vLLM
renders it at evaluation time (thinking disabled), and loss is computed on the
answer only. The test split is never read here.
"""

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import compact  # noqa: E402


def load(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines() if line.strip()]


def to_prompt_completion(records, tokenizer):
    """Template-rendered prompt and answer strings, as the model will see them when served."""
    rows = []
    for record in records:
        if record.get("split") == "test":
            raise SystemExit(f"refusing to train on a test record: {record['id']}")
        prompt = tokenizer.apply_chat_template(
            compact.messages(record), tokenize=False, add_generation_prompt=True, enable_thinking=False
        )
        rows.append({"prompt": prompt, "completion": compact.render_target(record) + tokenizer.eos_token})
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--model", default="Qwen/Qwen3-0.6B")
    parser.add_argument("--train", type=Path, required=True)
    parser.add_argument("--dev", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--epochs", type=float, default=3)
    parser.add_argument("--learning-rate", type=float, default=2e-5)
    parser.add_argument("--batch-size", type=int, default=8)
    parser.add_argument("--grad-accum", type=int, default=2)
    parser.add_argument("--max-length", type=int, default=2048)
    parser.add_argument("--seed", type=int, default=7)
    args = parser.parse_args()

    import torch
    from datasets import Dataset
    from transformers import AutoModelForCausalLM, AutoTokenizer
    from trl import SFTConfig, SFTTrainer

    tokenizer = AutoTokenizer.from_pretrained(args.model)
    train_rows = to_prompt_completion(load(args.train), tokenizer)
    dev_rows = to_prompt_completion(load(args.dev), tokenizer)
    lengths = sorted(len(tokenizer(row["prompt"] + row["completion"]).input_ids) for row in train_rows)
    print(json.dumps({
        "train_examples": len(train_rows),
        "dev_examples": len(dev_rows),
        "train_tokens": sum(lengths),
        "tokens_p50": lengths[len(lengths) // 2],
        "tokens_max": lengths[-1],
        "over_max_length": sum(length > args.max_length for length in lengths),
    }), flush=True)
    print("FIRST EXAMPLE >>>\n" + train_rows[0]["prompt"] + train_rows[0]["completion"] + "\n<<<", flush=True)

    model = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=torch.float32)
    config = SFTConfig(
        output_dir=str(args.output),
        num_train_epochs=args.epochs,
        learning_rate=args.learning_rate,
        lr_scheduler_type="cosine",
        warmup_ratio=0.03,
        per_device_train_batch_size=args.batch_size,
        per_device_eval_batch_size=args.batch_size,
        gradient_accumulation_steps=args.grad_accum,
        gradient_checkpointing=True,
        bf16=True,
        max_length=args.max_length,
        completion_only_loss=True,
        logging_steps=10,
        eval_strategy="epoch",
        save_strategy="no",
        report_to="none",
        seed=args.seed,
    )
    trainer = SFTTrainer(
        model=model,
        args=config,
        train_dataset=Dataset.from_list(train_rows),
        eval_dataset=Dataset.from_list(dev_rows),
        processing_class=tokenizer,
    )
    started = time.time()
    trainer.train()
    minutes = (time.time() - started) / 60
    trainer.save_model(str(args.output))
    tokenizer.save_pretrained(str(args.output))
    summary = {"model": args.model, "minutes": round(minutes, 1), "epochs": args.epochs, "learning_rate": args.learning_rate,
               "final_eval": trainer.evaluate(), "train_examples": len(train_rows)}
    (args.output / "sentinal_training.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print("TRAINING DONE " + json.dumps(summary), flush=True)


if __name__ == "__main__":
    main()
