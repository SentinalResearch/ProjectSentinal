#!/usr/bin/env python3
"""Produce deterministic training progress and a real tiny checkpoint file."""

from __future__ import annotations

import argparse
from pathlib import Path
import time


def positive_int(value: str) -> int:
    """Parse a strictly positive integer."""
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError(f"expected a positive integer, got {value!r}")
    return parsed


def non_negative_float(value: str) -> float:
    """Parse a non-negative delay."""
    parsed = float(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError(f"expected a non-negative number, got {value!r}")
    return parsed


def parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--epochs", type=positive_int, default=4)
    result.add_argument("--delay", type=non_negative_float, default=1.0)
    result.add_argument("--output-dir", type=Path, default=Path("artifacts"))
    return result


def run(epochs: int, delay: float, output_dir: Path) -> int:
    """Emit recognised training lines and persist one checkpoint."""
    checkpoint_epoch = max(1, (epochs + 1) // 2)
    print("Sentinal notification training demo started", flush=True)
    for epoch in range(1, epochs + 1):
        if delay:
            time.sleep(delay)
        loss = 1.0 / (epoch + 1)
        val_loss = loss + 0.05
        learning_rate = 0.001 / epoch
        print(
            f"Epoch {epoch}/{epochs} loss={loss:.4f} "
            f"val_loss={val_loss:.4f} lr={learning_rate:.6f}",
            flush=True,
        )
        if epoch == checkpoint_epoch:
            output_dir.mkdir(parents=True, exist_ok=True)
            checkpoint = output_dir / f"notification-demo-epoch-{epoch}.safetensors"
            checkpoint.write_bytes(b"sentinal-notification-demo\n")
            print(f"Saving checkpoint to {checkpoint}", flush=True)
    print("Sentinal notification training demo completed successfully", flush=True)
    return 0


def main() -> int:
    """Parse arguments and run the training acceptance scenario."""
    args = parser().parse_args()
    return run(args.epochs, args.delay, args.output_dir)


if __name__ == "__main__":
    raise SystemExit(main())
