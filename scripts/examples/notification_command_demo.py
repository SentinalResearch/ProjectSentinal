#!/usr/bin/env python3
"""Produce deterministic Command Events for production notification acceptance."""

from __future__ import annotations

import argparse
import sys
import time


def non_negative_float(value: str) -> float:
    """Parse a non-negative delay."""
    parsed = float(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError(f"expected a non-negative number, got {value!r}")
    return parsed


def parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("scenario", choices=("success", "failure", "waiting"))
    result.add_argument("--delay", type=non_negative_float, default=1.0)
    return result


def pause(delay: float) -> None:
    """Pause only when the requested manual-demo delay is non-zero."""
    if delay:
        time.sleep(delay)


def run(scenario: str, delay: float) -> int:
    """Run one scenario and return its intentional exit status."""
    print(f"Sentinal notification demo: {scenario}", flush=True)

    if scenario == "waiting":
        pause(delay)
        print("Continue? [y/N]", flush=True)
        response = sys.stdin.readline().strip().lower()
        if response in {"y", "yes"}:
            print("response accepted; command completed successfully", flush=True)
        else:
            print("response declined; command completed successfully", flush=True)
        return 0

    print("phase 1/2", flush=True)
    pause(delay)
    print("phase 2/2", flush=True)
    pause(delay)
    if scenario == "failure":
        # The exit itself should produce one failed-Command notification. Do
        # not also emit a generic error pattern and fabricate a second Event.
        print("intentional non-zero exit (status 7)", flush=True)
        return 7

    print("completed successfully", flush=True)
    return 0


def main() -> int:
    """Parse arguments and run the selected acceptance scenario."""
    args = parser().parse_args()
    return run(args.scenario, args.delay)


if __name__ == "__main__":
    raise SystemExit(main())
