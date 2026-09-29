#!/usr/bin/env python3
"""Deterministic long-running process for Sentinal handoff testing."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import socket
import sys
import time
from typing import Optional


def positive_int(value: str) -> int:
    """Parse a strictly positive integer argument."""
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError(f"expected a positive integer, got {value!r}")
    return parsed


def non_negative_float(value: str) -> float:
    """Parse a non-negative floating-point argument."""
    parsed = float(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError(f"expected a non-negative number, got {value!r}")
    return parsed


def parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--steps", type=positive_int, default=120)
    result.add_argument("--interval", type=non_negative_float, default=1.0)
    result.add_argument(
        "--fail-at",
        type=positive_int,
        help="exit with status 3 at this step to test a FAILED Session",
    )
    return result


def run(steps: int, interval: float, fail_at: Optional[int]) -> int:
    """Print observable progress and return the intended process exit code."""
    started = time.monotonic()
    print("Sentinal long-job demo started", flush=True)
    print(f"pid={os.getpid()} host={socket.gethostname()}", flush=True)
    print(f"cwd={Path.cwd()}", flush=True)
    print(f"steps={steps} interval={interval:.3f}s", flush=True)

    checkpoint_steps = {max(1, round(steps * fraction)) for fraction in (0.25, 0.5, 0.75)}
    for step in range(1, steps + 1):
        if interval:
            time.sleep(interval)
        elapsed = time.monotonic() - started
        percent = step * 100 / steps
        print(
            f"step={step:03d}/{steps:03d} progress={percent:6.2f}% elapsed={elapsed:7.2f}s",
            flush=True,
        )
        if step in checkpoint_steps:
            print(f"checkpoint reached at step {step}", flush=True)
        if fail_at == step:
            print(f"intentional failure at step {step}", file=sys.stderr, flush=True)
            return 3

    print(f"Sentinal long-job demo completed in {time.monotonic() - started:.2f}s", flush=True)
    return 0


def main() -> int:
    """Run the demo and map Ctrl-C to the conventional exit status 130."""
    args = parser().parse_args()
    if args.fail_at is not None and args.fail_at > args.steps:
        parser().error(f"--fail-at {args.fail_at} exceeds --steps {args.steps}")
    try:
        return run(args.steps, args.interval, args.fail_at)
    except KeyboardInterrupt:
        print("\nSentinal long-job demo interrupted by Ctrl-C", file=sys.stderr, flush=True)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
